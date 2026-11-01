//! The Perch control panel: a normal desktop window (feature `render`) on
//! the main display, drawn with egui, that is the home for everything you
//! would otherwise need the CLI for — picking windows, the layout grid,
//! focus/remove/span per window, calibration, input grab, sharing status and
//! quitting. It sends the same commands the CLI and phones send, so nothing
//! here bypasses the engine.

use crate::capture::UiMsg;
use egui_wgpu::ScreenDescriptor;
use perch_proto::{Command, Grid, State, WindowId};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId as WinitWindowId};

/// What the panel shows this frame (a snapshot taken under the engine lock).
pub struct PanelSnapshot {
    /// Full state as controllers see it.
    pub state: State,
    /// Render frame rate on the glasses.
    pub fps: f32,
    /// Current gaze (yaw, pitch) in radians, for "centre here".
    pub gaze: (f32, f32),
    /// Host name advertised on mDNS.
    pub host: String,
}

/// The panel window and its GPU state.
pub struct ControlPanel {
    window: Arc<Window>,
    egui_state: egui_winit::State,
    ctx: egui::Context,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    commands: mpsc::UnboundedSender<UiMsg>,
    grab: Arc<AtomicBool>,
    /// Grid being edited with the sliders (applied when a drag ends).
    grid_edit: Option<Grid>,
    /// Quit was requested.
    pub quit: bool,
}

impl ControlPanel {
    /// Create the window (on whatever output the compositor chooses, which
    /// is the active one: the laptop).
    pub fn new(
        el: &ActiveEventLoop,
        commands: mpsc::UnboundedSender<UiMsg>,
        grab: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        let attrs = Window::default_attributes()
            .with_title("Perch")
            .with_inner_size(winit::dpi::LogicalSize::new(440.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(360.0, 480.0));
        let window = Arc::new(el.create_window(attrs)?);
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow::anyhow!("no GPU adapter for the control panel"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let egui_state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
        Ok(Self {
            window,
            egui_state,
            ctx,
            device,
            queue,
            surface,
            config,
            renderer,
            commands,
            grab,
            grid_edit: None,
            quit: false,
        })
    }

    /// winit id of the panel window.
    pub fn id(&self) -> WinitWindowId {
        self.window.id()
    }

    /// Ask for a repaint.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Feed a window event. Returns true if it was a redraw request (the
    /// caller then supplies a snapshot via [`draw`](Self::draw)).
    pub fn on_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::Resized(s) => {
                self.config.width = s.width.max(1);
                self.config.height = s.height.max(1);
                self.surface.configure(&self.device, &self.config);
            }
            WindowEvent::CloseRequested => self.quit = true,
            WindowEvent::RedrawRequested => return true,
            _ => {}
        }
        let _ = self.egui_state.on_window_event(&self.window, event);
        false
    }

    fn send(&self, cmd: Command) {
        let _ = self.commands.send(UiMsg::Command(cmd));
    }

    /// Run the UI and present one frame.
    pub fn draw(&mut self, snap: &PanelSnapshot) {
        let raw = self.egui_state.take_egui_input(&self.window);
        let mut to_send: Vec<Command> = Vec::new();
        let mut grab_toggle = false;
        let mut quit = false;
        let egui_ctx = self.ctx.clone();
        let out = egui_ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.ui(ui, snap, &mut to_send, &mut grab_toggle, &mut quit);
            });
        });
        for cmd in to_send {
            self.send(cmd);
        }
        if grab_toggle {
            let now = !self.grab.load(Ordering::Relaxed);
            self.grab.store(now, Ordering::Relaxed);
        }
        if quit {
            self.quit = true;
        }
        self.egui_state
            .handle_platform_output(&self.window, out.platform_output);
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        for (id, delta) in &out.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, delta);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        let sd = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: out.pixels_per_point,
        };
        let extra = self
            .renderer
            .update_buffers(&self.device, &self.queue, &mut enc, &prims, &sd);
        {
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("perch-panel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.07,
                            g: 0.075,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &prims, &sd);
        }
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        self.queue.submit(extra.into_iter().chain([enc.finish()]));
        frame.present();
    }

    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        snap: &PanelSnapshot,
        out: &mut Vec<Command>,
        grab_toggle: &mut bool,
        quit: &mut bool,
    ) {
        let s = &snap.state;
        ui.horizontal(|ui| {
            ui.heading("Perch");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Quit").clicked() {
                    *quit = true;
                }
            });
        });
        ui.label(format!(
            "{}  ·  {}  ·  {:?}  ·  {:.0} fps",
            s.glasses.as_deref().unwrap_or("no glasses"),
            match s.calibration {
                perch_proto::CalibrationState::Ready => "calibrated".to_string(),
                perch_proto::CalibrationState::Calibrating { percent } => format!("calibrating {percent}%"),
                perch_proto::CalibrationState::Uncalibrated => "uncalibrated".to_string(),
            },
            s.display_mode.unwrap_or(perch_proto::DisplayMode::Mono),
            snap.fps
        ));
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            if ui.button("Pick windows…").clicked() {
                out.push(Command::PickSources { multiple: true });
            }
            if ui.button("Arrange").clicked() {
                out.push(Command::ArrangeGrid);
            }
            if ui.button("Recenter").clicked() {
                out.push(Command::Recenter);
            }
            if ui.button("Centre here").clicked() {
                out.push(Command::SetGrid {
                    grid: Grid {
                        center_yaw: snap.gaze.0,
                        center_pitch: snap.gaze.1,
                        ..s.grid
                    },
                });
            }
            if ui.button("Calibrate (30 s)").clicked() {
                out.push(Command::Calibrate { seconds: 30 });
            }
            if ui.button("Clear").clicked() {
                for w in &s.windows {
                    out.push(Command::RemoveWindow { id: w.id });
                }
            }
        });
        let grabbed = self.grab.load(Ordering::Relaxed);
        let mut g = grabbed;
        if ui
            .checkbox(&mut g, "Perch owns mouse & keyboard (Scroll Lock toggles)")
            .changed()
        {
            *grab_toggle = true;
        }
        ui.separator();

        ui.collapsing("Layout grid", |ui| {
            let grid = self.grid_edit.get_or_insert(s.grid);
            let mut changed = false;
            changed |= ui
                .add(egui::Slider::new(&mut grid.rows, 1..=4).text("rows"))
                .drag_stopped();
            changed |= ui
                .add(egui::Slider::new(&mut grid.distance, 0.8..=6.0).text("distance m"))
                .drag_stopped();
            changed |= ui
                .add(egui::Slider::new(&mut grid.row_h_m, 0.3..=2.0).text("row height m"))
                .drag_stopped();
            changed |= ui
                .add(egui::Slider::new(&mut grid.px_per_m, 600.0..=4000.0).text("px per m"))
                .drag_stopped();
            changed |= ui
                .add(egui::Slider::new(&mut grid.block_w_m, 0.2..=1.5).text("block width m"))
                .drag_stopped();
            changed |= ui
                .add(egui::Slider::new(&mut grid.gap_m, 0.0..=0.5).text("gap m"))
                .drag_stopped();
            if changed {
                out.push(Command::SetGrid { grid: *grid });
            }
            if ui.button("Reset grid").clicked() {
                *grid = Grid::default();
                out.push(Command::SetGrid { grid: *grid });
            }
        });
        // Keep the sliders in sync with changes made elsewhere when idle.
        if !ui.ctx().is_using_pointer() {
            self.grid_edit = Some(s.grid);
        }
        ui.separator();

        ui.label(format!("Windows ({})", s.windows.len()));
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for w in &s.windows {
                let focused = s.focused == Some(w.id);
                ui.horizontal(|ui| {
                    let name = source_name(&w.source);
                    let text = if focused {
                        egui::RichText::new(format!("● {name}")).strong()
                    } else {
                        egui::RichText::new(format!("○ {name}"))
                    };
                    ui.label(text);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Remove").clicked() {
                            out.push(Command::RemoveWindow { id: w.id });
                        }
                        if ui.small_button("Pin").on_hover_text("Pin where I'm looking").clicked() {
                            out.push(Command::PinHere { id: w.id });
                        }
                        if !focused && ui.small_button("Focus").clicked() {
                            out.push(Command::Focus { id: w.id });
                        }
                        span_combo(ui, w.id, out);
                    });
                });
                ui.label(
                    egui::RichText::new(format!(
                        "   yaw {:.0}°  pitch {:.0}°  {:.2}×{:.2} m  {:?}",
                        w.placement.yaw.to_degrees(),
                        w.placement.pitch.to_degrees(),
                        w.placement.width_m,
                        w.placement.height_m,
                        w.anchor
                    ))
                    .small()
                    .weak(),
                );
            }
        });
        ui.separator();

        ui.collapsing("Sharing", |ui| {
            ui.label(format!("This host: {}  (control :7420, stream :7421)", snap.host));
            ui.label("Peers:");
            for p in &s.peers {
                ui.label(format!("  {}  {:?}", p.name, p.platform));
            }
            ui.label(
                egui::RichText::new("Phones/tablets find this host via mDNS; see docs/PLATFORMS.md for pairing.")
                    .small()
                    .weak(),
            );
        });
    }
}

fn span_combo(ui: &mut egui::Ui, id: WindowId, out: &mut Vec<Command>) {
    let mut span: u32 = 0;
    egui::ComboBox::from_id_salt(("span", id))
        .selected_text("span")
        .width(70.0)
        .show_ui(ui, |ui| {
            for (v, label) in [
                (0, "auto"),
                (1, "1 block"),
                (2, "2 blocks"),
                (3, "3 blocks"),
                (4, "4 blocks"),
            ] {
                if ui.selectable_value(&mut span, v, label).clicked() {
                    out.push(Command::SetWindowSpan { id, span: v });
                }
            }
        });
}

fn source_name(s: &perch_proto::Source) -> String {
    use perch_proto::Source::*;
    match s {
        Display { display_index, .. } => format!("Monitor {display_index}"),
        Window { title, .. } if !title.is_empty() => title.clone(),
        Window { handle, .. } => format!("Window {handle}"),
        Virtual { width, height, .. } => format!("Demo {width}×{height}"),
        Dex { .. } => "Phone (DeX)".into(),
        PhoneScreen { .. } => "Phone screen".into(),
    }
}
