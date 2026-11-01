//! Render loop (feature `render`): opens a fullscreen window on the glasses
//! monitor and draws the workspace every frame. Must run on the main thread
//! (winit requirement); the tokio runtime runs on another thread.

use crate::engine::Engine;
use perch_capture::{CaptureSource, TestPattern};
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

/// Everything the render loop needs.
pub struct RenderApp {
    engine: Arc<Mutex<Engine>>,
    tracker: Arc<StdMutex<perch_tracking::Tracker>>,
    monitor_hint: String,
    stereo: Stereo,
    window: Option<Arc<Window>>,
    comp: Option<Compositor>,
    sources: HashMap<WindowId, Box<dyn CaptureSource>>,
    last_frame: Instant,
    /// Use the test pattern for every window (no capture backend yet; see M2).
    #[allow(dead_code)]
    pub test_pattern: bool,
}

impl RenderApp {
    /// New app.
    pub fn new(
        engine: Arc<Mutex<Engine>>,
        tracker: Arc<StdMutex<perch_tracking::Tracker>>,
        monitor_hint: String,
        ipd_m: f32,
    ) -> Self {
        Self {
            engine,
            tracker,
            monitor_hint,
            stereo: Stereo::SideBySide { ipd_m },
            window: None,
            comp: None,
            sources: HashMap::new(),
            last_frame: Instant::now(),
            test_pattern: true,
        }
    }

    /// Run the winit loop (blocks the main thread).
    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        event_loop.run_app(&mut self)?;
        Ok(())
    }

    fn ensure_sources(&mut self, windows: &[perch_proto::SpatialWindow]) {
        for w in windows {
            self.sources.entry(w.id).or_insert_with(|| {
                let (cw, ch) = match &w.source {
                    Source::Virtual { width, height, .. } => (*width, *height),
                    _ => (1920, 1080),
                };
                // TODO(M2): replace with perch_capture::default_backend().open(&w.source, 60)
                // for local sources and a network frame receiver for remote peers.
                Box::new(TestPattern::new(w.source.clone(), cw.min(1920), ch.min(1200), 60))
            });
        }
        let live: Vec<WindowId> = windows.iter().map(|w| w.id).collect();
        self.sources.retain(|id, _| live.contains(id));
    }

    fn frame(&mut self) {
        if self.comp.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        // Predicted pose for this frame.
        let pose = self.tracker.lock().unwrap().pose_predicted(0.0);

        let (windows, projector, mode) = {
            let mut e = self.engine.blocking_lock();
            e.set_pose(pose);
            (e.windows_for_render(dt), e.projector, e.display_mode)
        };
        self.ensure_sources(&windows);
        let comp = self.comp.as_mut().expect("checked above");

        // Upload new frames.
        for (id, src) in self.sources.iter_mut() {
            if let Ok(Some(f)) = src.next_frame(std::time::Duration::from_millis(0)) {
                comp.upload(*id, &f);
            }
        }

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
                let left: Vec<_> = base.iter().map(|q| shift(q, 0.5)).collect();
                let right: Vec<_> = base
                    .iter()
                    .map(|q| {
                        let mut r = shift(q, -0.5);
                        for c in &mut r.corners {
                            c.x += projector.optics.width_px as f32;
                        }
                        r
                    })
                    .collect();
                comp.render_stereo(&left, &right)
            } else {
                comp.render(&projector.project_all(&ws_tmp, &pose))
            }
        } else {
            comp.render(&projector.project_all(&ws_tmp, &pose))
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
            None => tracing::warn!(
                "no monitor matching {:?}; rendering in a normal window",
                self.monitor_hint
            ),
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
                self.frame();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
