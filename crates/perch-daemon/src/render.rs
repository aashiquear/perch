//! Render loop (feature `render`): opens a fullscreen window on the glasses
//! monitor and draws the workspace every frame. Must run on the main thread
//! (winit requirement); the tokio runtime runs on another thread.

use crate::capture::SharedBackend;
use crate::engine::Engine;
use perch_capture::{CaptureError, CaptureSource, TestPattern};
use perch_compositor::{find_glasses_monitor, Compositor, Stereo};
use perch_proto::{DisplayMode, Source, WindowId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::sync::Mutex;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Fullscreen, Window, WindowId as WinitWindowId};

/// How the render loop gets pixels for local windows.
pub struct CaptureSetup {
    /// Platform backend (None → test pattern everywhere).
    pub backend: SharedBackend,
    /// Our peer id: only sources owned by us are opened locally.
    pub me: uuid::Uuid,
    /// Requested capture rate.
    pub fps: u32,
}

/// Everything the render loop needs.
pub struct RenderApp {
    engine: Arc<Mutex<Engine>>,
    tracker: Arc<StdMutex<perch_tracking::Tracker>>,
    monitor_hint: String,
    stereo: Stereo,
    window: Option<Arc<Window>>,
    comp: Option<Compositor>,
    sources: HashMap<WindowId, Box<dyn CaptureSource>>,
    /// Windows whose capture failed to open, with the time of the last try;
    /// they show the test pattern and are retried every few seconds (e.g.
    /// while a restore is still in flight).
    failed: HashMap<WindowId, Instant>,
    capture: CaptureSetup,
    last_frame: Instant,
    stats: FrameStats,
    /// Pointer position on the glasses panel while the OS pointer is on our
    /// output (GNOME lets it wander off the laptop screen; we hide the OS
    /// cursor and draw our own so the user can find it and bring it back).
    cursor: Option<(f32, f32)>,
    cursor_uploaded: bool,
}

/// Layer id for the pointer overlay.
const CURSOR_ID: WindowId = uuid::Uuid::from_u128(0x7065_7263_685f_6375_7273_6f72_0000_0001);
const CURSOR_W: u32 = 24;
const CURSOR_H: u32 = 32;

/// A white arrow with a black outline, as an RGBA frame.
fn cursor_frame() -> perch_capture::Frame {
    let (w, h) = (CURSOR_W as i32, CURSOR_H as i32);
    let mut data = vec![0u8; (w * h * 4) as usize];
    // Arrow polygon in a 24×32 box: tip top-left, classic shape.
    let inside = |x: f32, y: f32| -> bool {
        let poly = [
            (1.0, 1.0),
            (1.0, 24.0),
            (7.0, 19.0),
            (11.0, 29.0),
            (15.0, 27.0),
            (11.0, 17.0),
            (19.0, 17.0),
        ];
        let mut c = false;
        let n = poly.len();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
                c = !c;
            }
            j = i;
        }
        c
    };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let i = ((y * w + x) * 4) as usize;
            if inside(fx, fy) {
                data[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            } else {
                // Outline: any neighbour inside.
                let edge = [
                    (-1.0, 0.0),
                    (1.0, 0.0),
                    (0.0, -1.0),
                    (0.0, 1.0),
                    (-1.0, -1.0),
                    (1.0, 1.0),
                    (-1.0, 1.0),
                    (1.0, -1.0),
                ]
                .iter()
                .any(|(dx, dy)| inside(fx + dx, fy + dy));
                if edge {
                    data[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
    perch_capture::Frame::Rgba {
        width: CURSOR_W,
        height: CURSOR_H,
        stride: CURSOR_W * 4,
        format: perch_capture::PixelFormat::Rgba8,
        data: data.into(),
        t_us: 0,
    }
}

fn with_cursor(mut quads: Vec<perch_space::ScreenQuad>, cursor: Option<(f32, f32)>) -> Vec<perch_space::ScreenQuad> {
    if let Some((x, y)) = cursor {
        quads.push(cursor_quad(x, y, 0.0));
    }
    quads
}

/// Screen-space quad for the pointer at panel pixel (x, y), drawn on top.
fn cursor_quad(x: f32, y: f32, x_shift: f32) -> perch_space::ScreenQuad {
    use nalgebra::Point2;
    let (w, h) = (CURSOR_W as f32, CURSOR_H as f32);
    let x = x + x_shift;
    perch_space::ScreenQuad {
        id: CURSOR_ID,
        corners: [
            Point2::new(x, y),
            Point2::new(x + w, y),
            Point2::new(x + w, y + h),
            Point2::new(x, y + h),
        ],
        depth: 0.0,
        z: i32::MAX,
        opacity: 1.0,
        on_screen: true,
    }
}

/// Rolling frame-time statistics, logged every few seconds so "it feels
/// slow" can be checked against numbers.
#[derive(Default)]
struct FrameStats {
    frames: u32,
    total_s: f32,
    max_s: f32,
    since: Option<Instant>,
}

impl FrameStats {
    fn add(&mut self, dt: f32) {
        let now = Instant::now();
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        self.total_s += dt;
        self.max_s = self.max_s.max(dt);
        if now.duration_since(since).as_secs() >= 5 {
            let fps = self.frames as f32 / now.duration_since(since).as_secs_f32();
            tracing::info!(
                fps = format_args!("{fps:.0}"),
                avg_ms = format_args!("{:.1}", 1000.0 * self.total_s / self.frames.max(1) as f32),
                max_ms = format_args!("{:.1}", 1000.0 * self.max_s),
                "render"
            );
            *self = Self {
                since: Some(now),
                ..Default::default()
            };
        }
    }
}

impl RenderApp {
    /// New app.
    pub fn new(
        engine: Arc<Mutex<Engine>>,
        tracker: Arc<StdMutex<perch_tracking::Tracker>>,
        monitor_hint: String,
        ipd_m: f32,
        capture: CaptureSetup,
    ) -> Self {
        Self {
            engine,
            tracker,
            monitor_hint,
            stereo: Stereo::SideBySide { ipd_m },
            window: None,
            comp: None,
            sources: HashMap::new(),
            failed: HashMap::new(),
            capture,
            last_frame: Instant::now(),
            stats: FrameStats::default(),
            cursor: None,
            cursor_uploaded: false,
        }
    }

    /// Open a capture for `w`, or a test pattern if that isn't possible.
    /// `None` means "the backend is busy (a pick dialog is open); try again
    /// next frame" — never block the render thread on it.
    fn open_source(&mut self, w: &perch_proto::SpatialWindow) -> Option<Box<dyn CaptureSource>> {
        let (cw, ch) = match &w.source {
            Source::Virtual { width, height, .. } => (*width, *height),
            _ => (1920, 1080),
        };
        let local = match &w.source {
            Source::Display { peer, .. } | Source::Window { peer, .. } => *peer == self.capture.me,
            _ => false,
        };
        if local {
            let res = match self.capture.backend.try_lock() {
                Ok(mut guard) => match guard.as_mut() {
                    Some(b) => b.open(&w.source, self.capture.fps),
                    None => Err(CaptureError::Unavailable("no capture backend".into())),
                },
                Err(std::sync::TryLockError::WouldBlock) => return None,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    Err(CaptureError::Other("capture backend poisoned".into()))
                }
            };
            match res {
                Ok(src) => {
                    self.failed.remove(&w.id);
                    return Some(src);
                }
                Err(e) => {
                    if !self.failed.contains_key(&w.id) {
                        tracing::warn!(window = %w.id, "capture unavailable, showing test pattern: {e}");
                    }
                    self.failed.insert(w.id, Instant::now());
                }
            }
        }
        // TODO(M4): network frame receiver for remote peers.
        Some(Box::new(TestPattern::new(
            w.source.clone(),
            cw.min(1920),
            ch.min(1200),
            60,
        )))
    }

    /// Run the winit loop (blocks the main thread).
    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        event_loop.run_app(&mut self)?;
        Ok(())
    }

    fn ensure_sources(&mut self, windows: &[perch_proto::SpatialWindow]) {
        const RETRY: std::time::Duration = std::time::Duration::from_secs(3);
        for w in windows {
            let retry = self.failed.get(&w.id).is_some_and(|t| t.elapsed() > RETRY);
            if !self.sources.contains_key(&w.id) || retry {
                if let Some(src) = self.open_source(w) {
                    self.sources.insert(w.id, src);
                }
            }
        }
        let live: Vec<WindowId> = windows.iter().map(|w| w.id).collect();
        self.sources.retain(|id, _| live.contains(id));
        self.failed.retain(|id, _| live.contains(id));
    }

    fn frame(&mut self) {
        if self.comp.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.stats.add(dt);

        // Predicted pose for this frame.
        let pose = self.tracker.lock().unwrap().pose_predicted(0.0);

        let (windows, projector, mode) = {
            let mut e = self.engine.blocking_lock();
            e.set_pose(pose);
            (e.windows_for_render(dt), e.projector, e.display_mode)
        };
        self.ensure_sources(&windows);
        let comp = self.comp.as_mut().expect("checked above");

        // Upload new frames; a stream that ended is reopened next frame.
        let mut ended = Vec::new();
        for (id, src) in self.sources.iter_mut() {
            match src.next_frame(std::time::Duration::from_millis(0)) {
                Ok(Some(f)) => comp.upload(*id, &f),
                Ok(None) => {}
                Err(e) => {
                    tracing::info!(window = %id, "capture ended: {e}");
                    ended.push(*id);
                }
            }
        }
        for id in ended {
            self.sources.remove(&id);
            self.failed
                .insert(id, Instant::now() - std::time::Duration::from_secs(60));
        }

        if self.cursor.is_some() && !self.cursor_uploaded {
            comp.upload(CURSOR_ID, &cursor_frame());
            self.cursor_uploaded = true;
        }
        let cursor = self.cursor;
        let stereo = matches!(mode, Some(DisplayMode::Stereo) | Some(DisplayMode::Stereo120));
        let ws_tmp = perch_space::Workspace {
            windows: windows.clone(),
            ..Default::default()
        };
        let res = if stereo {
            if let Stereo::SideBySide { ipd_m } = self.stereo {
                // Parallax: shift each eye's view by ±ipd/2 horizontally. For a
                // 3DOF renderer this is a yaw-independent translation of the
                // projected quad by f * (ipd/2) / distance pixels.
                let f = projector.optics.focal_px();
                let base = projector.project_all(&ws_tmp, &pose);
                let shift = |q: &perch_space::ScreenQuad, sign: f32| {
                    let mut q = *q;
                    let dx = sign * f * (ipd_m / 2.0) / q.depth.max(0.1);
                    for c in &mut q.corners {
                        c.x += dx;
                    }
                    q
                };
                let mut left: Vec<_> = base.iter().map(|q| shift(q, 0.5)).collect();
                let mut right: Vec<_> = base
                    .iter()
                    .map(|q| {
                        let mut r = shift(q, -0.5);
                        for c in &mut r.corners {
                            c.x += projector.optics.width_px as f32;
                        }
                        r
                    })
                    .collect();
                if let Some((x, y)) = cursor {
                    // The OS pointer lives on the full panel; show it in the eye it is in.
                    let half = projector.optics.width_px as f32 / 2.0;
                    let (xl, shift_r) = if x < half { (x, half) } else { (x - half, half) };
                    left.push(cursor_quad(xl, y, 0.0));
                    right.push(cursor_quad(xl, y, shift_r));
                }
                comp.render_stereo(&left, &right)
            } else {
                comp.render(&with_cursor(projector.project_all(&ws_tmp, &pose), cursor))
            }
        } else {
            comp.render(&with_cursor(projector.project_all(&ws_tmp, &pose), cursor))
        };
        if let Err(e) = res {
            tracing::warn!("render: {e}");
            let (w, h) = comp.size();
            comp.resize(w, h);
        }
    }
}

impl ApplicationHandler for RenderApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let monitor = find_glasses_monitor(el.available_monitors(), &self.monitor_hint);
        match &monitor {
            Some(m) => tracing::info!(name = ?m.name(), size = ?m.size(), "rendering on glasses monitor"),
            None => {
                // On Wayland winit reports connector names ("DP-5"), not the
                // EDID product string, so the default hint "Rokid" won't match;
                // list what we saw so the user can set `display.monitor_name`.
                let seen: Vec<String> = el.available_monitors().filter_map(|m| m.name()).collect();
                tracing::warn!(
                    "no monitor matching {:?} (available: {:?}); rendering in a normal window — set display.monitor_name",
                    self.monitor_hint,
                    seen
                );
            }
        }
        let attrs = Window::default_attributes()
            .with_title("Perch")
            .with_decorations(false)
            .with_fullscreen(monitor.clone().map(|m| Fullscreen::Borderless(Some(m))));
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        window.set_cursor_visible(false);
        match Compositor::new(Arc::clone(&window)) {
            Ok(c) => self.comp = Some(c),
            Err(e) => tracing::error!("compositor init failed: {e}"),
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WinitWindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(c) = self.comp.as_mut() {
                    c.resize(s.width, s.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let t0 = Instant::now();
                self.frame();
                // Mailbox presentation never blocks, so pace ourselves: ~2×
                // the 120 Hz panel is plenty and keeps CPU/GPU idle otherwise.
                const MIN_FRAME: std::time::Duration = std::time::Duration::from_micros(4000);
                if let Some(left) = MIN_FRAME.checked_sub(t0.elapsed()) {
                    std::thread::sleep(left);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
