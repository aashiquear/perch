//! Render loop (feature `render`): opens a fullscreen window on the glasses
//! monitor and draws the workspace every frame. Must run on the main thread
//! (winit requirement); the tokio runtime runs on another thread.

use crate::capture::SharedBackend;
use crate::engine::Engine;
use crate::interact::{FrameCtx, Interaction};
use crate::panel::{ControlPanel, PanelSnapshot};
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
    /// Where pointer/bar interactions send their commands and forwarded input.
    pub commands: tokio::sync::mpsc::UnboundedSender<crate::capture::UiMsg>,
    /// Set while Perch owns the physical mouse/keyboard.
    pub grab: Arc<std::sync::atomic::AtomicBool>,
    /// Physical input events while grabbed.
    pub device_rx: Option<std::sync::mpsc::Receiver<perch_proto::InputEvent>>,
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
    /// Pointer, drag and control-bar state.
    interact: Interaction,
    /// What the last frame used, for hit-testing pointer events.
    ctx: Option<FrameCtx>,
    /// Desktop control window.
    panel: Option<ControlPanel>,
    panel_last: Instant,
    /// Last check that the glasses window still sits on its monitor.
    monitor_check: Instant,
    was_grabbed: bool,
    host: String,
    last_fps: f32,
}

/// Rolling frame-time statistics, logged every few seconds so "it feels
/// slow" can be checked against numbers.
#[derive(Default)]
struct FrameStats {
    frames: u32,
    total_s: f32,
    max_s: f32,
    since: Option<Instant>,
    /// Most recently logged fps.
    last_fps: f32,
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
                last_fps: fps,
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
        let capture_commands = capture.commands.clone();
        let grab = Arc::clone(&capture.grab);
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
            interact: Interaction::new(capture_commands, grab),
            ctx: None,
            panel: None,
            panel_last: Instant::now(),
            monitor_check: Instant::now(),
            was_grabbed: false,
            host: String::new(),
            last_fps: 0.0,
        }
    }

    /// Host name shown in the panel's sharing section.
    pub fn with_host(mut self, host: String) -> Self {
        self.host = host;
        self
    }

    fn draw_panel(&mut self) {
        let Some(panel) = self.panel.as_mut() else { return };
        let snap = {
            let e = self.engine.blocking_lock();
            PanelSnapshot {
                state: e.state(),
                fps: self.last_fps,
                gaze: perch_space::gaze_angles(&e.pose),
                host: self.host.clone(),
            }
        };
        panel.draw(&snap);
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
        self.last_fps = self.stats.last_fps;

        // Grab flips: re-sync the OS pointer under the spatial one.
        let grabbed_now = self.interact.grabbed();
        if grabbed_now != self.was_grabbed {
            self.was_grabbed = grabbed_now;
            if let Some(ctx) = self.ctx.clone() {
                self.interact.grab_changed(grabbed_now, &ctx);
            }
        }
        // Physical input while grabbed drives the spatial pointer.
        if let (Some(rx), Some(ctx)) = (self.capture.device_rx.as_ref(), self.ctx.clone()) {
            let batch: Vec<perch_proto::InputEvent> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
            if !batch.is_empty() {
                let mut e = self.engine.blocking_lock();
                self.interact.device_events(&batch, &ctx, &mut e);
            }
        }

        // Predicted pose for this frame.
        let pose = self.tracker.lock().unwrap().pose_predicted(0.0);

        let (windows, projector, mode, grid, drag_overlay, sizes) = {
            let mut e = self.engine.blocking_lock();
            e.set_pose(pose);
            let drag_overlay = self.interact.drag_overlay(&e);
            (
                e.windows_for_render(dt),
                e.projector,
                e.display_mode,
                e.ws.grid,
                drag_overlay,
                e.ws.sizes.clone(),
            )
        };
        self.ensure_sources(&windows);
        let comp = self.comp.as_mut().expect("checked above");

        // Upload new frames; a stream that ended is reopened next frame.
        let mut ended = Vec::new();
        let mut resized = Vec::new();
        for (id, src) in self.sources.iter_mut() {
            match src.next_frame(std::time::Duration::from_millis(0)) {
                Ok(Some(f)) => {
                    comp.upload(*id, &f);
                    // A window stream's visible size is only known from its
                    // pixels (GNOME streams are monitor-sized); keep the grid
                    // sized to what is actually shown.
                    let px = src.size();
                    let known = sizes.get(id).copied();
                    let differs = known.is_none_or(|k| k.0.abs_diff(px.0) > 8 || k.1.abs_diff(px.1) > 8);
                    if px.0 > 0 && px.1 > 0 && differs {
                        resized.push((*id, px));
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::info!(window = %id, "capture ended: {e}");
                    ended.push(*id);
                }
            }
        }
        for (id, (width, height)) in resized {
            let _ = self
                .capture
                .commands
                .send(crate::capture::UiMsg::Command(perch_proto::Command::SetWindowPixels {
                    id,
                    width,
                    height,
                }));
        }
        for id in ended {
            self.sources.remove(&id);
            self.failed
                .insert(id, Instant::now() - std::time::Duration::from_secs(60));
        }

        let stereo = matches!(mode, Some(DisplayMode::Stereo) | Some(DisplayMode::Stereo120));
        let ws_tmp = perch_space::Workspace {
            windows,
            ..Default::default()
        };
        // Real windows first (these are what the pointer can grab) …
        let base = projector.project_all(&ws_tmp, &pose);
        let panel = (projector.optics.width_px, projector.optics.height_px);
        self.ctx = Some(FrameCtx {
            pose,
            projector,
            panel,
            stereo,
            quads: base.clone(),
        });
        // … then world-anchored overlay (drag ghosts + guide) …
        let mut world = base;
        for g in &drag_overlay {
            if let Some(q) = projector.project(g, &pose) {
                world.push(q);
            }
        }
        // … then screen-space overlay (bar, pointer) per eye.
        let ctx = self.ctx.clone().expect("just set");
        let n_windows = ctx.quads.len() as u32;
        let screen = self.interact.screen_overlay(&ctx, n_windows, &grid, comp);
        let res = if stereo {
            if let Stereo::SideBySide { ipd_m } = self.stereo {
                // Parallax: shift each eye's view by ±ipd/2 horizontally. For a
                // 3DOF renderer this is a yaw-independent translation of the
                // projected quad by f * (ipd/2) / distance pixels.
                let f = projector.optics.focal_px();
                let shift = |q: &perch_space::ScreenQuad, sign: f32| {
                    let mut q = *q;
                    let dx = sign * f * (ipd_m / 2.0) / q.depth.max(0.1);
                    for c in &mut q.corners {
                        c.x += dx;
                    }
                    q
                };
                let half = projector.optics.width_px as f32 / 2.0;
                let mut left: Vec<_> = world.iter().map(|q| shift(q, 0.5)).collect();
                let mut right: Vec<_> = world
                    .iter()
                    .map(|q| {
                        let mut r = shift(q, -0.5);
                        for c in &mut r.corners {
                            c.x += half;
                        }
                        r
                    })
                    .collect();
                for q in &screen {
                    left.push(*q);
                    let mut r = *q;
                    for c in &mut r.corners {
                        c.x += half;
                    }
                    right.push(r);
                }
                comp.render_stereo(&left, &right)
            } else {
                world.extend(screen);
                comp.render(&world)
            }
        } else {
            world.extend(screen);
            comp.render(&world)
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
        match ControlPanel::new(el, self.capture.commands.clone(), Arc::clone(&self.capture.grab)) {
            Ok(p) => self.panel = Some(p),
            Err(e) => tracing::warn!("control panel unavailable: {e}"),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WinitWindowId, event: WindowEvent) {
        if let WindowEvent::Focused(f) = event {
            crate::capture::PERCH_FOCUSED.store(f, std::sync::atomic::Ordering::Relaxed);
        }
        if self.panel.as_ref().is_some_and(|p| p.id() == id) {
            let redraw = self.panel.as_mut().expect("checked").on_event(&event);
            if redraw {
                self.draw_panel();
            }
            if self.panel.as_ref().is_some_and(|p| p.quit) {
                tracing::info!("quit from control panel");
                self.engine.blocking_lock().save();
                el.exit();
            }
            return;
        }
        match event {
            // Alt+F4 or a WM close must never take the glasses view down
            // (shortcuts Perch injects could land here); quit from the panel.
            WindowEvent::CloseRequested => tracing::info!("ignoring close request on the glasses window"),
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
            // While Perch owns the physical devices the spatial pointer is
            // driven by evdev only; the OS pointer our own injection moves
            // must not echo back into it through the compositor.
            WindowEvent::CursorMoved { .. } | WindowEvent::CursorLeft { .. } | WindowEvent::MouseInput { .. }
                if self.interact.grabbed() => {}
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(ctx) = self.ctx.clone() {
                    let mut e = self.engine.blocking_lock();
                    self.interact
                        .cursor_moved(position.x as f32, position.y as f32, &ctx, &mut e);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                let mut e = self.engine.blocking_lock();
                self.interact.cursor_left(&mut e);
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                if let Some(ctx) = self.ctx.clone() {
                    let mut e = self.engine.blocking_lock();
                    self.interact
                        .button(state == winit::event::ElementState::Pressed, &ctx, &mut e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        // Monitor hot-plug (virtual glasses displays come and go) can knock
        // the fullscreen window off the glasses; put it back.
        if self.monitor_check.elapsed() >= std::time::Duration::from_secs(2) {
            self.monitor_check = Instant::now();
            if let Some(w) = &self.window {
                let want = find_glasses_monitor(el.available_monitors(), &self.monitor_hint);
                if let Some(want) = want {
                    let on_it = w.current_monitor().is_some_and(|m| m.name() == want.name());
                    let fullscreen = w.fullscreen().is_some();
                    if !on_it || !fullscreen {
                        tracing::info!(name = ?want.name(), on_it, fullscreen, "re-pinning glasses window to its monitor");
                        w.set_fullscreen(Some(Fullscreen::Borderless(Some(want))));
                    }
                }
            }
        }
        // The panel is a desktop window: 20 Hz is plenty.
        if self.panel_last.elapsed() >= std::time::Duration::from_millis(50) {
            self.panel_last = Instant::now();
            if let Some(p) = &self.panel {
                p.request_redraw();
            }
        }
    }
}
