//! Pointer interaction on the glasses (feature `render`): the OS pointer
//! wanders onto our output, and while it is here the user can click a
//! window to focus it, drag it to re-order the grid (with a live insertion
//! guide and ghost outlines of where the row would settle), or use the
//! control bar along the bottom. Everything geometric is delegated to
//! `perch-space` / `perch-ui`; this module only turns winit events into
//! engine calls and commands.

use crate::engine::Engine;
use perch_capture::{Frame, PixelFormat};
use perch_proto::{Anchor, Command, Placement, Pose, SpatialWindow, WindowId};
use perch_space::{Projector, ScreenQuad};
use perch_ui::{Bar, BarAction};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Layer ids for overlay textures (never collide with window UUIDs).
pub const CURSOR_ID: WindowId = Uuid::from_u128(0x7065_7263_685f_6375_7273_6f72_0000_0001);
const GHOST_ID: WindowId = Uuid::from_u128(0x7065_7263_685f_6375_7273_6f72_0000_0002);
const GUIDE_ID: WindowId = Uuid::from_u128(0x7065_7263_685f_6375_7273_6f72_0000_0003);
const BAR_ID: WindowId = Uuid::from_u128(0x7065_7263_685f_6375_7273_6f72_0000_0004);

const CURSOR_W: u32 = 24;
const CURSOR_H: u32 = 32;
/// Pointer must move this far before a press becomes a drag.
const DRAG_THRESHOLD_PX: f32 = 6.0;

/// What the render loop knows about the current frame; events need it to
/// turn panel pixels into world directions.
#[derive(Clone)]
pub struct FrameCtx {
    /// Pose used for the last frame.
    pub pose: Pose,
    /// Projector used for the last frame.
    pub projector: Projector,
    /// Panel size in pixels.
    pub panel: (u32, u32),
    /// Side-by-side stereo: the view is half the panel wide.
    pub stereo: bool,
    /// Projected real windows from the last frame (mono/view coordinates,
    /// painter's order, so the last containing quad is on top).
    pub quads: Vec<ScreenQuad>,
}

impl FrameCtx {
    fn view_w(&self) -> f32 {
        if self.stereo {
            self.panel.0 as f32 / 2.0
        } else {
            self.panel.0 as f32
        }
    }

    /// Panel pixel → view pixel (folds the right eye onto the left).
    fn view_xy(&self, x: f32, y: f32) -> (f32, f32) {
        if self.stereo && x >= self.view_w() {
            (x - self.view_w(), y)
        } else {
            (x, y)
        }
    }

    fn topmost_window_at(&self, x: f32, y: f32) -> Option<WindowId> {
        let p = nalgebra::Point2::new(x, y);
        self.quads
            .iter()
            .rev()
            .find(|q| perch_space::quad_contains(&q.corners, p))
            .map(|q| q.id)
    }
}

struct Press {
    id: WindowId,
    x: f32,
    y: f32,
}

struct Drag {
    id: WindowId,
    /// Window centre minus pointer direction at grab time, so the window
    /// doesn't jump to the pointer.
    dyaw: f32,
    dpitch: f32,
}

/// Interaction state owned by the render loop.
pub struct Interaction {
    /// Pointer on the panel, if it is on our output.
    pub cursor: Option<(f32, f32)>,
    bar: Bar,
    bar_hover: Option<usize>,
    bar_pressed: Option<usize>,
    /// (view width, hover, pressed, status) of the bar texture last uploaded.
    bar_uploaded: Option<(u32, Option<usize>, Option<usize>, String)>,
    textures_uploaded: bool,
    press: Option<Press>,
    drag: Option<Drag>,
    commands: mpsc::UnboundedSender<Command>,
}

impl Interaction {
    /// New, with the channel that delivers commands to the engine loop.
    pub fn new(commands: mpsc::UnboundedSender<Command>) -> Self {
        Self {
            cursor: None,
            bar: Bar::default(),
            bar_hover: None,
            bar_pressed: None,
            bar_uploaded: None,
            textures_uploaded: false,
            press: None,
            drag: None,
            commands,
        }
    }

    fn bar_top(&self, ctx: &FrameCtx) -> f32 {
        ctx.panel.1 as f32 - self.bar.height() as f32
    }

    /// Bar button index under the pointer, if any.
    fn bar_hit(&self, ctx: &FrameCtx, x: f32, y: f32) -> Option<usize> {
        let (vx, vy) = ctx.view_xy(x, y);
        let top = self.bar_top(ctx);
        if vy < top {
            return None;
        }
        self.bar.hit(ctx.view_w() as u32, vx, vy - top)
    }

    /// Pointer moved to panel pixel (`x`, `y`).
    pub fn cursor_moved(&mut self, x: f32, y: f32, ctx: &FrameCtx, engine: &mut Engine) {
        self.cursor = Some((x, y));
        self.bar_hover = self.bar_hit(ctx, x, y);
        if let Some(p) = &self.press {
            if self.drag.is_none() && ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt() > DRAG_THRESHOLD_PX {
                let id = p.id;
                let (vx, vy) = ctx.view_xy(p.x, p.y);
                let (yaw, pitch) = ctx.projector.unproject(vx, vy, &ctx.pose);
                if let Some(w) = engine.ws.get(id) {
                    let (dyaw, dpitch) = (perch_space::wrap_pi(w.placement.yaw - yaw), w.placement.pitch - pitch);
                    engine.ws.begin_drag(id);
                    self.drag = Some(Drag { id, dyaw, dpitch });
                }
            }
        }
        if let Some(d) = &self.drag {
            let (vx, vy) = ctx.view_xy(x, y);
            let (yaw, pitch) = ctx.projector.unproject(vx, vy, &ctx.pose);
            let _ = engine.ws.drag_update(d.id, yaw + d.dyaw, pitch + d.dpitch);
        }
    }

    /// Pointer left our output.
    pub fn cursor_left(&mut self, engine: &mut Engine) {
        self.cursor = None;
        self.bar_hover = None;
        self.bar_pressed = None;
        self.finish_drag(engine);
        self.press = None;
    }

    /// Primary button went down / up.
    pub fn button(&mut self, down: bool, ctx: &FrameCtx, engine: &mut Engine) {
        let Some((x, y)) = self.cursor else { return };
        if down {
            if let Some(i) = self.bar_hit(ctx, x, y) {
                self.bar_pressed = Some(i);
                return;
            }
            let (vx, vy) = ctx.view_xy(x, y);
            if let Some(id) = ctx.topmost_window_at(vx, vy) {
                self.press = Some(Press { id, x, y });
                if engine.ws.focused != Some(id) {
                    let _ = self.commands.send(Command::Focus { id });
                }
            }
        } else {
            if let Some(i) = self.bar_pressed.take() {
                if self.bar_hit(ctx, x, y) == Some(i) {
                    let action = self.bar.buttons[i].action;
                    self.run(action, engine);
                }
            }
            self.finish_drag(engine);
            self.press = None;
        }
    }

    /// Drop the dragged window: into the row the preview shows, or free.
    fn finish_drag(&mut self, engine: &mut Engine) {
        let Some(d) = self.drag.take() else { return };
        let Some(w) = engine.ws.get(d.id) else { return };
        let (yaw, pitch, placement) = (w.placement.yaw, w.placement.pitch, w.placement);
        let cmd = match engine.ws.drag_preview(d.id, yaw, pitch) {
            Some(p) => Command::SlotWindow {
                id: d.id,
                row: p.row,
                index: p.index,
            },
            None => Command::PlaceWindow { id: d.id, placement },
        };
        let _ = self.commands.send(cmd);
    }

    fn run(&mut self, action: BarAction, engine: &Engine) {
        let grid = engine.ws.grid;
        let cmd = match action {
            BarAction::Recenter => Command::Recenter,
            BarAction::Arrange => Command::ArrangeGrid,
            BarAction::Pick => Command::PickSources { multiple: true },
            BarAction::Calibrate => Command::Calibrate { seconds: 30 },
            BarAction::RowsMinus => Command::SetGrid {
                grid: perch_proto::Grid {
                    rows: grid.rows.saturating_sub(1).max(1),
                    ..grid
                },
            },
            BarAction::RowsPlus => Command::SetGrid {
                grid: perch_proto::Grid {
                    rows: (grid.rows + 1).min(4),
                    ..grid
                },
            },
            BarAction::Nearer => Command::SetGrid {
                grid: perch_proto::Grid {
                    distance: (grid.distance - 0.25).max(0.8),
                    ..grid
                },
            },
            BarAction::Farther => Command::SetGrid {
                grid: perch_proto::Grid {
                    distance: (grid.distance + 0.25).min(6.0),
                    ..grid
                },
            },
            BarAction::CentreHere => {
                let (yaw, pitch) = perch_space::gaze_angles(&engine.pose);
                Command::SetGrid {
                    grid: perch_proto::Grid {
                        center_yaw: yaw,
                        center_pitch: pitch,
                        ..grid
                    },
                }
            }
            BarAction::Clear => {
                for w in &engine.ws.windows {
                    let _ = self.commands.send(Command::RemoveWindow { id: w.id });
                }
                return;
            }
        };
        let _ = self.commands.send(cmd);
    }

    /// Ghost outlines + insertion guide for the current drag (world
    /// anchored), computed while the engine is locked.
    pub fn drag_overlay(&self, engine: &Engine) -> Vec<SpatialWindow> {
        let Some(d) = &self.drag else { return Vec::new() };
        let Some(w) = engine.ws.get(d.id) else {
            return Vec::new();
        };
        let Some(p) = engine.ws.drag_preview(d.id, w.placement.yaw, w.placement.pitch) else {
            return Vec::new();
        };
        let ghost = |id: WindowId, placement: Placement, z: i32| SpatialWindow {
            id,
            source: w.source.clone(),
            placement,
            anchor: Anchor::World,
            z,
            opacity: 0.9,
            visible: true,
            focused: false,
        };
        let mut v: Vec<SpatialWindow> = p
            .ghosts
            .iter()
            .map(|(_, pl)| ghost(GHOST_ID, *pl, i32::MAX - 2))
            .collect();
        v.push(ghost(GUIDE_ID, p.guide, i32::MAX - 1));
        v
    }

    /// Screen-space overlay (bar, pointer) in view coordinates, uploading
    /// textures as needed.
    pub fn screen_overlay(
        &mut self,
        ctx: &FrameCtx,
        engine_grid_rows: u32,
        grid_distance: f32,
        comp: &mut perch_compositor::Compositor,
    ) -> Vec<ScreenQuad> {
        if !self.textures_uploaded {
            comp.upload(CURSOR_ID, &cursor_frame());
            comp.upload(GHOST_ID, &outline_frame());
            comp.upload(GUIDE_ID, &solid_frame([120, 190, 255, 230]));
            self.textures_uploaded = true;
        }
        let view_w = ctx.view_w() as u32;
        let status = format!("{} ROWS  {:.1} M", engine_grid_rows, grid_distance);
        let key = (view_w, self.bar_hover, self.bar_pressed, status.clone());
        if self.bar_uploaded.as_ref() != Some(&key) {
            let data = self.bar.render(view_w, self.bar_hover, self.bar_pressed, &status);
            comp.upload(
                BAR_ID,
                &Frame::Rgba {
                    width: view_w,
                    height: self.bar.height(),
                    stride: view_w * 4,
                    format: PixelFormat::Rgba8,
                    data: data.into(),
                    t_us: 0,
                },
            );
            self.bar_uploaded = Some(key);
        }
        let mut v = Vec::new();
        let top = self.bar_top(ctx);
        v.push(screen_quad(
            BAR_ID,
            0.0,
            top,
            view_w as f32,
            self.bar.height() as f32,
            i32::MAX - 1,
        ));
        if let Some((x, y)) = self.cursor {
            let (vx, vy) = ctx.view_xy(x, y);
            v.push(screen_quad(
                CURSOR_ID,
                vx,
                vy,
                CURSOR_W as f32,
                CURSOR_H as f32,
                i32::MAX,
            ));
        }
        v
    }
}

fn screen_quad(id: WindowId, x: f32, y: f32, w: f32, h: f32, z: i32) -> ScreenQuad {
    use nalgebra::Point2;
    ScreenQuad {
        id,
        corners: [
            Point2::new(x, y),
            Point2::new(x + w, y),
            Point2::new(x + w, y + h),
            Point2::new(x, y + h),
        ],
        depth: 0.0,
        z,
        opacity: 1.0,
        on_screen: true,
    }
}

fn solid_frame(rgba: [u8; 4]) -> Frame {
    let data: Vec<u8> = rgba.iter().copied().cycle().take(4 * 4 * 4).collect();
    Frame::Rgba {
        width: 4,
        height: 4,
        stride: 16,
        format: PixelFormat::Rgba8,
        data: data.into(),
        t_us: 0,
    }
}

/// 64×64 transparent square with a 3-px light border: stretched over a
/// ghost placement it outlines where a window would land.
fn outline_frame() -> Frame {
    const N: usize = 64;
    let mut data = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let edge = x < 3 || y < 3 || x >= N - 3 || y >= N - 3;
            if edge {
                let i = (y * N + x) * 4;
                data[i..i + 4].copy_from_slice(&[120, 190, 255, 220]);
            }
        }
    }
    Frame::Rgba {
        width: N as u32,
        height: N as u32,
        stride: (N * 4) as u32,
        format: PixelFormat::Rgba8,
        data: data.into(),
        t_us: 0,
    }
}

/// A white arrow with a black outline, as an RGBA frame.
fn cursor_frame() -> Frame {
    let (w, h) = (CURSOR_W as i32, CURSOR_H as i32);
    let mut data = vec![0u8; (w * h * 4) as usize];
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
            let (xi, yi): (f32, f32) = poly[i];
            let (xj, yj): (f32, f32) = poly[j];
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
    Frame::Rgba {
        width: CURSOR_W,
        height: CURSOR_H,
        stride: CURSOR_W * 4,
        format: PixelFormat::Rgba8,
        data: data.into(),
        t_us: 0,
    }
}
