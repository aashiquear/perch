//! The Perch control panel: a normal desktop window (feature `render`) on
//! the main display, drawn with egui. It is a dashboard — the connected
//! glasses, live stats, a map of the layout — plus big icon buttons for
//! everything the glasses bar and the hotkeys do, so nothing needs the CLI.
//! It sends the same commands the CLI and phones send, so nothing here
//! bypasses the engine.

use crate::capture::UiMsg;
use egui_wgpu::ScreenDescriptor;
use perch_proto::{Command, Grid, State, WindowId};
use perch_ui::icons::{self, Icon, ICON};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/// Product shot of the glasses we know, shown in the header once detected.
/// Rokid Max: Rokid's own product image (au.rokid.com), used only to
/// identify the connected device.
const ROKID_MAX_PNG: &[u8] = include_bytes!("../../../assets/glasses/rokid-max.png");

/// Full-size glasses display.
const FULL: (u32, u32) = (1920, 1080);
/// Half-width glasses display.
const HALF: (u32, u32) = (960, 1080);

/// Layout presets for the "Arrange" menu: rows, then the display width per
/// window in list order (2 = full 1920×1080, 1 = half 960×1080). A window
/// whose display is already that width keeps it; the others are re-created
/// at the new size, so a full window keeps its aspect and the narrow ones
/// become genuinely half as wide.
const PRESETS: &[(&str, &str, u32, &[u32])] = &[
    ("1 × 1", "one full-size window", 1, &[2]),
    ("3 equal", "three full-size windows in a row", 1, &[2, 2, 2]),
    ("wide centre", "half · full · half", 1, &[1, 2, 1]),
    ("wide left", "full · half · half", 1, &[2, 1, 1]),
    ("wide right", "half · half · full", 1, &[1, 1, 2]),
    ("2 × 2", "two rows of two full-size windows", 2, &[2, 2, 2, 2]),
];

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
    icons: HashMap<Icon, egui::TextureHandle>,
    glasses_img: Option<egui::TextureHandle>,
    /// "To glasses" clicked: fires once the panel loses focus to the app
    /// the user clicks (clicking the button itself focused the panel).
    pending_move: Option<(u32, u32)>,
    move_fire_at: Option<Instant>,
    arrange_open: bool,
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
            .with_inner_size(winit::dpi::LogicalSize::new(600.0, 860.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(480.0, 600.0));
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
        let icons = all_icons()
            .into_iter()
            .map(|icon| {
                (
                    icon,
                    ctx.load_texture(format!("icon-{icon:?}"), icon_image(icon), egui::TextureOptions::LINEAR),
                )
            })
            .collect();
        let glasses_img = match decode_png(ROKID_MAX_PNG) {
            Ok(img) => Some(ctx.load_texture("glasses-rokid-max", img, egui::TextureOptions::LINEAR)),
            Err(e) => {
                tracing::warn!("glasses image: {e}");
                None
            }
        };
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
            icons,
            glasses_img,
            pending_move: None,
            move_fire_at: None,
            arrange_open: false,
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
            // The user clicked the app they want on the glasses: it has
            // focus now, so the move can go ahead.
            WindowEvent::Focused(false) if self.pending_move.is_some() => {
                self.move_fire_at = Some(Instant::now() + Duration::from_millis(300));
            }
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
        if let (Some(size), Some(at)) = (self.pending_move, self.move_fire_at) {
            if Instant::now() >= at {
                self.pending_move = None;
                self.move_fire_at = None;
                self.send(Command::MoveToGlasses {
                    width: size.0,
                    height: size.1,
                });
            }
        }
        let raw = self.egui_state.take_egui_input(&self.window);
        let mut to_send: Vec<Command> = Vec::new();
        let mut grab_toggle = false;
        let mut quit = false;
        let egui_ctx = self.ctx.clone();
        let out = egui_ctx.run(raw, |ctx| {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.pending_move = None;
                self.move_fire_at = None;
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.ui(ui, snap, &mut to_send, &mut grab_toggle, &mut quit);
                });
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

    fn icon(&self, icon: Icon, size: f32) -> egui::Image<'_> {
        let tex = &self.icons[&icon];
        egui::Image::new((tex.id(), egui::vec2(size, size)))
    }

    /// A square icon-only button with its name and keyboard shortcut to the
    /// right. Returns true when clicked.
    fn action(&self, ui: &mut egui::Ui, icon: Icon, label: &str, shortcut: &str, hint: &str, selected: bool) -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            let btn = egui::Button::image(self.icon(icon, 30.0))
                .min_size(egui::vec2(52.0, 52.0))
                .selected(selected);
            clicked = ui.add(btn).on_hover_text(hint).clicked();
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(label).size(15.0));
                if !shortcut.is_empty() {
                    ui.label(egui::RichText::new(shortcut).small().weak());
                }
            });
        });
        clicked
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
        let grabbed = self.grab.load(Ordering::Relaxed);

        // ---- header: glasses picture, name, mode -------------------------
        ui.horizontal(|ui| {
            if let (Some(tex), Some(name)) = (&self.glasses_img, s.glasses.as_deref()) {
                if name.to_ascii_lowercase().contains("rokid") {
                    let sz = tex.size_vec2();
                    let h = 96.0;
                    ui.add(egui::Image::new((tex.id(), egui::vec2(sz.x / sz.y * h, h))));
                }
            }
            ui.vertical(|ui| {
                ui.heading(egui::RichText::new("Perch").size(26.0));
                match s.glasses.as_deref() {
                    Some(g) => {
                        ui.label(egui::RichText::new(g).size(16.0).strong());
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  ·  {}",
                                s.display_mode
                                    .map(|m| format!("{m:?}"))
                                    .unwrap_or_else(|| "mode ?".into()),
                                match s.calibration {
                                    perch_proto::CalibrationState::Ready => "calibrated".to_string(),
                                    perch_proto::CalibrationState::Calibrating { percent } =>
                                        format!("calibrating {percent}%"),
                                    perch_proto::CalibrationState::Uncalibrated => "not calibrated".to_string(),
                                }
                            ))
                            .weak(),
                        );
                    }
                    None => {
                        ui.label(egui::RichText::new("No glasses connected").color(egui::Color32::LIGHT_RED));
                    }
                }
                let (txt, col) = if grabbed {
                    (
                        "● GLASSES MODE — mouse & keys go to the glasses",
                        egui::Color32::from_rgb(90, 220, 140),
                    )
                } else {
                    (
                        "○ LAPTOP MODE — glasses only display",
                        egui::Color32::from_rgb(230, 190, 80),
                    )
                };
                ui.label(egui::RichText::new(txt).color(col).strong());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui
                    .button("Quit")
                    .on_hover_text("Stop Perch; apps return to the laptop")
                    .clicked()
                {
                    *quit = true;
                }
            });
        });
        if self.pending_move.is_some() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 210, 90),
                "➜  Now click the window you want on the glasses (Esc cancels)",
            );
        }
        ui.add_space(6.0);

        // ---- stat tiles ----------------------------------------------------
        let rows_used = {
            let mut rows: Vec<i32> = s
                .windows
                .iter()
                .map(|w| ((w.placement.pitch - s.grid.center_pitch) * 100.0).round() as i32)
                .collect();
            rows.sort_unstable();
            rows.dedup();
            rows.len()
        };
        ui.horizontal(|ui| {
            stat(ui, &s.windows.len().to_string(), "windows");
            stat(
                ui,
                &format!(
                    "{} / {}",
                    rows_used.max(usize::from(!s.windows.is_empty())),
                    s.grid.rows
                ),
                "rows used",
            );
            stat(ui, &format!("{:.1} m", s.grid.distance), "distance");
            stat(ui, &format!("{:.0}", snap.fps), "fps");
            stat(ui, &s.peers.len().to_string(), "peers");
        });
        ui.add_space(6.0);

        // ---- layout map ----------------------------------------------------
        layout_map(ui, s);
        ui.add_space(6.0);

        // ---- actions: square icon buttons, name + shortcut beside ----------
        ui.columns(2, |c| {
            if self.action(
                &mut c[0],
                Icon::Pick,
                "To glasses",
                "Ctrl+Alt+G in the app",
                "Then click the app window to move.",
                self.pending_move == Some(FULL),
            ) {
                self.pending_move = Some(FULL);
                self.move_fire_at = None;
            }
            if self.action(
                &mut c[1],
                Icon::Resize,
                "To glasses, half width",
                "Ctrl+Alt+H in the app",
                "Then click the app window to move; 960×1080 display.",
                self.pending_move == Some(HALF),
            ) {
                self.pending_move = Some(HALF);
                self.move_fire_at = None;
            }
        });
        ui.columns(2, |c| {
            if self.action(
                &mut c[0],
                Icon::Grab,
                if grabbed { "Back to laptop mode" } else { "Glasses mode" },
                "Ctrl+Alt+P",
                "Mouse & keyboard go to the glasses while on.",
                grabbed,
            ) {
                *grab_toggle = true;
            }
            if self.action(
                &mut c[1],
                Icon::ToLaptop,
                "All to laptop",
                "",
                "Remove every window; apps return to the laptop",
                false,
            ) {
                for w in &s.windows {
                    out.push(Command::RemoveWindow { id: w.id });
                }
            }
        });
        ui.columns(2, |c| {
            if self.action(
                &mut c[0],
                Icon::Recenter,
                "Recenter",
                "temple button",
                "Current gaze becomes straight ahead",
                false,
            ) {
                out.push(Command::Recenter);
            }
            if self.action(
                &mut c[1],
                Icon::CentreHere,
                "Centre grid here",
                "",
                "Put the grid where you are looking",
                false,
            ) {
                out.push(Command::SetGrid {
                    grid: Grid {
                        center_yaw: snap.gaze.0,
                        center_pitch: snap.gaze.1,
                        ..s.grid
                    },
                });
            }
        });
        ui.columns(2, |c| {
            if self.action(
                &mut c[0],
                Icon::Arrange,
                if self.arrange_open {
                    "Arrange ▴"
                } else {
                    "Arrange ▾"
                },
                "presets below",
                "Re-flow the grid, or pick a preset",
                self.arrange_open,
            ) {
                self.arrange_open = !self.arrange_open;
            }
            if self.action(
                &mut c[1],
                Icon::Calibrate,
                "Calibrate (30 s)",
                "",
                "Glasses still on a table",
                false,
            ) {
                out.push(Command::Calibrate { seconds: 30 });
            }
        });
        if self.arrange_open {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_sized([60.0, 44.0], egui::Button::new("re-flow"))
                    .on_hover_text("Keep sizes, pack rows")
                    .clicked()
                {
                    out.push(Command::ArrangeGrid);
                }
                for (name, hint, rows, widths) in PRESETS {
                    if preset_button(ui, *rows, widths)
                        .on_hover_text(format!("{name}: {hint}"))
                        .clicked()
                    {
                        out.extend(preset_commands(s, *rows, widths));
                    }
                }
            });
            ui.label(
                egui::RichText::new(
                    "Widths apply to windows in list order: full keeps its aspect, half gets a 960-wide display.",
                )
                .small()
                .weak(),
            );
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Brightness: use the rocker on the glasses' right temple. The Rokid Max driver exposes no brightness control yet.",
            )
            .small()
            .weak(),
        );
        ui.separator();

        // ---- windows -----------------------------------------------------------
        ui.label(
            egui::RichText::new(format!("Windows ({})", s.windows.len()))
                .size(16.0)
                .strong(),
        );
        if s.windows.is_empty() {
            ui.label(egui::RichText::new("None yet. Focus an app and press Ctrl+Alt+G.").weak());
        }
        for (i, w) in s.windows.iter().enumerate() {
            let focused = s.focused == Some(w.id);
            egui::Frame::group(ui.style())
                .fill(if focused {
                    egui::Color32::from_rgb(30, 42, 52)
                } else {
                    egui::Color32::from_rgb(24, 26, 32)
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let name = format!("{}  {}", i + 1, source_name(&w.source));
                        ui.label(if focused {
                            egui::RichText::new(format!("● {name}")).strong()
                        } else {
                            egui::RichText::new(format!("○ {name}"))
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button("✕ Close app")
                                .on_hover_text("Close the application, then remove the window")
                                .clicked()
                            {
                                out.push(Command::CloseAppWindow { id: w.id });
                            }
                            if ui
                                .button("⇦ To laptop")
                                .on_hover_text("Remove the window; the app returns to the laptop")
                                .clicked()
                            {
                                out.push(Command::RemoveWindow { id: w.id });
                            }
                            if !focused && ui.button("Focus").clicked() {
                                out.push(Command::Focus { id: w.id });
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        size_combo(ui, w.id, out);
                        span_combo(ui, w.id, out);
                        if ui
                            .small_button("Pin here")
                            .on_hover_text("Pin where I'm looking")
                            .clicked()
                        {
                            out.push(Command::PinHere { id: w.id });
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "yaw {:.0}°  pitch {:.0}°  {:.2}×{:.2} m",
                                w.placement.yaw.to_degrees(),
                                w.placement.pitch.to_degrees(),
                                w.placement.width_m,
                                w.placement.height_m,
                            ))
                            .small()
                            .weak(),
                        );
                    });
                });
        }
        ui.separator();

        ui.collapsing("Layout grid (fine tuning)", |ui| {
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
        if !ui.ctx().is_using_pointer() {
            self.grid_edit = Some(s.grid);
        }

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

/// One stat tile: big number, small caption.
fn stat(ui: &mut egui::Ui, value: &str, caption: &str) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(24, 26, 32))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(value).size(20.0).strong());
                ui.label(egui::RichText::new(caption).small().weak());
            });
        });
}

/// Top-down-ish map of the spatial windows: yaw → x, pitch → y, angular
/// size → box size. The focused window is highlighted.
fn layout_map(ui: &mut egui::Ui, s: &State) {
    let width = ui.available_width();
    let height = 150.0;
    let (resp, p) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let r = resp.rect;
    p.rect_filled(r, 6.0, egui::Color32::from_rgb(18, 20, 26));
    // 110° of yaw across the width; same scale vertically.
    let k = width / 110f32.to_radians();
    let (cx, cy) = (r.center().x, r.center().y);
    let (cyaw, cpitch) = (s.grid.center_yaw, s.grid.center_pitch);
    // Centre cross.
    p.line_segment(
        [egui::pos2(cx, r.top() + 4.0), egui::pos2(cx, r.bottom() - 4.0)],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)),
    );
    p.line_segment(
        [egui::pos2(r.left() + 4.0, cy), egui::pos2(r.right() - 4.0, cy)],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(45)),
    );
    for (i, w) in s.windows.iter().enumerate() {
        let d = w.placement.distance.max(0.1);
        let aw = 2.0 * (w.placement.width_m / 2.0 / d).atan();
        let ah = 2.0 * (w.placement.height_m / 2.0 / d).atan();
        let x = cx - (w.placement.yaw - cyaw) * k;
        let y = cy - (w.placement.pitch - cpitch) * k;
        let rect = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(aw * k, ah * k));
        let rect = rect.intersect(r.shrink(2.0));
        let focused = s.focused == Some(w.id);
        let fill = if focused {
            egui::Color32::from_rgb(70, 140, 200)
        } else {
            egui::Color32::from_rgb(60, 70, 90)
        };
        p.rect_filled(rect.expand(1.0), 3.0, egui::Color32::from_gray(140));
        p.rect_filled(rect, 3.0, fill);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            (i + 1).to_string(),
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
    }
    if s.windows.is_empty() {
        p.text(
            egui::pos2(cx, cy),
            egui::Align2::CENTER_CENTER,
            "no windows on the glasses",
            egui::FontId::proportional(13.0),
            egui::Color32::from_gray(110),
        );
    }
}

/// Commands that apply a preset: rows, then width + slot per window in
/// list order; windows beyond the preset keep flowing automatically.
/// A small picture of the preset: boxes proportional to the widths.
fn preset_button(ui: &mut egui::Ui, rows: u32, widths: &[u32]) -> egui::Response {
    let size = egui::vec2(72.0, 44.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() {
        egui::Color32::from_rgb(50, 56, 70)
    } else {
        egui::Color32::from_rgb(34, 37, 46)
    };
    p.rect_filled(rect, 4.0, bg);
    let per_row = widths.len().div_ceil(rows as usize).max(1);
    let inner = rect.shrink(4.0);
    let row_h = inner.height() / rows as f32;
    for r in 0..rows as usize {
        let row: Vec<u32> = widths.iter().skip(r * per_row).take(per_row).copied().collect();
        let total: u32 = row.iter().sum::<u32>().max(1);
        let mut x = inner.left();
        for w in row {
            let ww = inner.width() * w as f32 / total as f32;
            let cell = egui::Rect::from_min_size(egui::pos2(x, inner.top() + r as f32 * row_h), egui::vec2(ww, row_h))
                .shrink(1.5);
            p.rect_filled(cell, 2.0, egui::Color32::from_rgb(120, 170, 220));
            x += ww;
        }
    }
    resp
}

/// Commands that apply a preset: rows, then per window in list order the
/// display width (re-created only when it differs), automatic span and
/// its slot. Windows beyond the preset keep flowing automatically.
fn preset_commands(s: &State, rows: u32, widths: &[u32]) -> Vec<Command> {
    let mut out = vec![Command::SetGrid {
        grid: Grid { rows, ..s.grid },
    }];
    let per_row = widths.len().div_ceil(rows as usize).max(1);
    for (i, w) in s.windows.iter().enumerate() {
        if let Some(&want) = widths.get(i) {
            // A half display is 960×1080 (aspect 0.89); full is 16:9.
            let aspect = w.placement.width_m / w.placement.height_m.max(0.01);
            let is_half = aspect < 1.2;
            if (want == 1) != is_half {
                let (width, height) = if want == 1 { HALF } else { FULL };
                out.push(Command::ResizeDisplay {
                    id: w.id,
                    width,
                    height,
                });
            }
        }
        out.push(Command::SetWindowSpan { id: w.id, span: 0 });
        out.push(Command::SlotWindow {
            id: w.id,
            row: ((i / per_row) as u32).min(rows - 1),
            index: (i % per_row) as u32,
        });
    }
    out
}

fn all_icons() -> Vec<Icon> {
    vec![
        Icon::Recenter,
        Icon::CentreHere,
        Icon::Arrange,
        Icon::RowsMinus,
        Icon::RowsPlus,
        Icon::Nearer,
        Icon::Farther,
        Icon::Pick,
        Icon::Calibrate,
        Icon::Clear,
        Icon::Grab,
        Icon::ToLaptop,
        Icon::CloseApp,
        Icon::Resize,
    ]
}

/// Rasterise a bar icon at 2× (48 px) for the panel buttons.
fn icon_image(icon: Icon) -> egui::ColorImage {
    let m = icons::mask(icon);
    let n = ICON * 2;
    let mut img = egui::ColorImage::new([n, n], egui::Color32::TRANSPARENT);
    for y in 0..n {
        for x in 0..n {
            let a = m[y / 2][x / 2];
            if a > 0 {
                img.pixels[y * n + x] = egui::Color32::from_rgba_unmultiplied(235, 238, 245, a);
            }
        }
    }
    img
}

fn decode_png(bytes: &[u8]) -> anyhow::Result<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba,
        "expected RGBA png, got {:?}",
        info.color_type
    );
    let (w, h) = (info.width as usize, info.height as usize);
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &buf[..w * h * 4]))
}

fn size_combo(ui: &mut egui::Ui, id: WindowId, out: &mut Vec<Command>) {
    let mut pick: Option<(u32, u32)> = None;
    egui::ComboBox::from_id_salt(("size", id))
        .selected_text("display size")
        .show_ui(ui, |ui| {
            for (label, w, h) in [
                ("full 1920×1080", 1920u32, 1080u32),
                ("half 960×1080", 960, 1080),
                ("wide 2560×1080", 2560, 1080),
                ("tall 1080×1440", 1080, 1440),
            ] {
                if ui.selectable_label(false, label).clicked() {
                    pick = Some((w, h));
                }
            }
        });
    if let Some((width, height)) = pick {
        out.push(Command::ResizeDisplay { id, width, height });
    }
}

fn span_combo(ui: &mut egui::Ui, id: WindowId, out: &mut Vec<Command>) {
    let mut span: u32 = 0;
    egui::ComboBox::from_id_salt(("span", id))
        .selected_text("width")
        .width(70.0)
        .show_ui(ui, |ui| {
            for (v, label) in [
                (0, "auto"),
                (1, "narrow (1 block)"),
                (2, "normal (2 blocks)"),
                (3, "wide (3 blocks)"),
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
        Display { display_index, .. } => format!("Glasses display {}", display_index + 1),
        Window { title, .. } if !title.is_empty() => title.clone(),
        Window { handle, .. } => format!("Window {handle}"),
        Virtual { width, height, .. } => format!("Demo {width}×{height}"),
        Dex { .. } => "Phone (DeX)".into(),
        PhoneScreen { .. } => "Phone screen".into(),
    }
}
