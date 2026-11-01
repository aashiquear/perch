//! The control bar: a compact pill of icon buttons at the bottom centre of
//! the glasses panel, the hovered button's name above it, and a status chip
//! (clock, window count, rows, distance, who owns the mouse) at the right.
//! Pure: layout and hit-testing are integer maths, rendering writes an RGBA
//! buffer the compositor uploads as a layer. Everything outside the pill and
//! the chip is transparent (black on the glasses).

use crate::font::{draw_text, text_width, GLYPH_H};
use crate::icons::{self, Icon, ICON};

/// What a button does when clicked. The daemon maps these to commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    /// Make the current gaze yaw = 0 (recalibrate the view here).
    Recenter,
    /// Re-flow all windows into the grid.
    Arrange,
    /// Open the source picker.
    Pick,
    /// One fewer grid row.
    RowsMinus,
    /// One more grid row.
    RowsPlus,
    /// Bring the layout closer.
    Nearer,
    /// Push the layout away.
    Farther,
    /// Move the layout centre to the current gaze (pitch and yaw).
    CentreHere,
    /// Run the 30 s stationary gyro calibration.
    Calibrate,
    /// Remove all windows.
    Clear,
    /// Move the focused app back to the laptop (removes its window).
    ToLaptop,
    /// Close the focused app and remove its window.
    CloseApp,
    /// Toggle the focused window's display between full and half width.
    Resize,
    /// Toggle: Perch owns the physical mouse/keyboard (spatial pointer on
    /// the glasses) vs. the OS does.
    Grab,
}

/// A button's icon, label and action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    /// Name shown above the pill while hovered.
    pub label: &'static str,
    /// Icon.
    pub icon: Icon,
    /// What it does.
    pub action: BarAction,
}

/// Rectangle in bar-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left.
    pub x: u32,
    /// Top (relative to the bar's own top edge).
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl Rect {
    /// Point inside?
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x as f32 && px < (self.x + self.w) as f32 && py >= self.y as f32 && py < (self.y + self.h) as f32
    }
}

/// Live values shown on the status chip.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarInfo {
    /// e.g. "14:05  SAT 1 NOV".
    pub clock: String,
    /// Open windows.
    pub windows: u32,
    /// Grid rows.
    pub rows: u32,
    /// Layout distance in centimetres (integer so the status is stable).
    pub distance_cm: u32,
    /// Perch owns the mouse/keyboard.
    pub grabbed: bool,
}

const TILE: u32 = 44;
const TILE_GAP: u32 = 6;
const PILL_PAD: u32 = 8;
const LABEL_SCALE: usize = 2;
const LABEL_H: u32 = (GLYPH_H * LABEL_SCALE) as u32 + 10;
const BOTTOM_MARGIN: u32 = 10;
const CHIP_SCALE: usize = 2;

/// The bar.
#[derive(Debug, Clone)]
pub struct Bar {
    /// Buttons, left to right.
    pub buttons: Vec<Button>,
}

impl Default for Bar {
    fn default() -> Self {
        let b = |label, icon, action| Button { label, icon, action };
        Self {
            buttons: vec![
                b("RECENTER", Icon::Recenter, BarAction::Recenter),
                b("CENTRE HERE", Icon::CentreHere, BarAction::CentreHere),
                b("ARRANGE", Icon::Arrange, BarAction::Arrange),
                b("FEWER ROWS", Icon::RowsMinus, BarAction::RowsMinus),
                b("MORE ROWS", Icon::RowsPlus, BarAction::RowsPlus),
                b("NEARER", Icon::Nearer, BarAction::Nearer),
                b("FARTHER", Icon::Farther, BarAction::Farther),
                b(
                    "TO GLASSES: LAPTOP'S FOCUSED APP (CTRL+ALT+G)",
                    Icon::Pick,
                    BarAction::Pick,
                ),
                b("TO LAPTOP: FOCUSED WINDOW", Icon::ToLaptop, BarAction::ToLaptop),
                b("FULL / HALF WIDTH: FOCUSED WINDOW", Icon::Resize, BarAction::Resize),
                b("CLOSE APP: FOCUSED WINDOW", Icon::CloseApp, BarAction::CloseApp),
                b("CALIBRATE", Icon::Calibrate, BarAction::Calibrate),
                b("ALL TO LAPTOP", Icon::Clear, BarAction::Clear),
                b("GLASSES MODE: MOUSE + KEYS (CTRL+ALT+P)", Icon::Grab, BarAction::Grab),
            ],
        }
    }
}

impl Bar {
    /// Bar strip height in pixels (label row + pill + margin).
    pub fn height(&self) -> u32 {
        LABEL_H + TILE + 2 * PILL_PAD + BOTTOM_MARGIN
    }

    /// Pill rectangle (bar-local).
    fn pill(&self, panel_w: u32) -> Rect {
        let n = self.buttons.len() as u32;
        let w = n * TILE + (n - 1) * TILE_GAP + 2 * PILL_PAD;
        Rect {
            x: panel_w.saturating_sub(w) / 2,
            y: LABEL_H,
            w,
            h: TILE + 2 * PILL_PAD,
        }
    }

    /// Button rectangles for a bar `panel_w` wide, centred.
    pub fn layout(&self, panel_w: u32) -> Vec<Rect> {
        let pill = self.pill(panel_w);
        (0..self.buttons.len() as u32)
            .map(|i| Rect {
                x: pill.x + PILL_PAD + i * (TILE + TILE_GAP),
                y: pill.y + PILL_PAD,
                w: TILE,
                h: TILE,
            })
            .collect()
    }

    /// Which button is under (`px`, `py`), in bar-local pixels?
    pub fn hit(&self, panel_w: u32, px: f32, py: f32) -> Option<usize> {
        self.layout(panel_w).iter().position(|r| r.contains(px, py))
    }

    /// Render the bar as RGBA (`panel_w` × `height()`).
    pub fn render(&self, panel_w: u32, hover: Option<usize>, pressed: Option<usize>, info: &BarInfo) -> Vec<u8> {
        let h = self.height();
        let (w, hh) = (panel_w as usize, h as usize);
        let mut buf = vec![0u8; w * hh * 4];
        // Pill.
        let pill = self.pill(panel_w);
        rounded_rect(&mut buf, w, hh, pill, 14, [16, 18, 24, 215]);
        rounded_outline(&mut buf, w, hh, pill, 14, [70, 78, 96, 255]);
        for (i, (b, r)) in self.buttons.iter().zip(self.layout(panel_w)).enumerate() {
            let active = b.action == BarAction::Grab && info.grabbed;
            let fill: Option<[u8; 4]> = if pressed == Some(i) {
                Some([90, 160, 255, 255])
            } else if hover == Some(i) {
                Some([58, 66, 84, 255])
            } else if active {
                Some([40, 90, 150, 255])
            } else {
                None
            };
            if let Some(f) = fill {
                rounded_rect(&mut buf, w, hh, r, 9, f);
            }
            let colour = if pressed == Some(i) {
                [255, 255, 255, 255]
            } else {
                [225, 230, 240, 255]
            };
            let ix = r.x as i32 + (TILE as i32 - ICON as i32) / 2;
            let iy = r.y as i32 + (TILE as i32 - ICON as i32) / 2;
            icons::blit(&mut buf, w, hh, ix, iy, &icons::mask(b.icon), colour);
            if hover == Some(i) {
                // Name above the pill, centred on the button.
                let tw = text_width(b.label, LABEL_SCALE) as u32 + 16;
                let lx = (r.x + TILE / 2).saturating_sub(tw / 2).min(panel_w.saturating_sub(tw));
                let lr = Rect {
                    x: lx,
                    y: 0,
                    w: tw,
                    h: LABEL_H - 4,
                };
                rounded_rect(&mut buf, w, hh, lr, 6, [16, 18, 24, 230]);
                draw_text(
                    &mut buf,
                    w,
                    hh,
                    lx as i32 + 8,
                    5,
                    b.label,
                    LABEL_SCALE,
                    [235, 235, 240, 255],
                );
            }
        }
        // Status chip at the right, vertically aligned with the pill.
        let status = format!(
            "{}   {} WIN  {} ROW  {}.{} M  {}",
            info.clock,
            info.windows,
            info.rows,
            info.distance_cm / 100,
            (info.distance_cm % 100) / 10,
            if info.grabbed { "PERCH" } else { "OS" }
        );
        let tw = text_width(&status, CHIP_SCALE) as u32 + 20;
        let th = (GLYPH_H * CHIP_SCALE) as u32 + 14;
        let cx = panel_w.saturating_sub(tw + 12);
        let cy = pill.y + (pill.h - th) / 2;
        let chip = Rect {
            x: cx,
            y: cy,
            w: tw,
            h: th,
        };
        rounded_rect(&mut buf, w, hh, chip, 8, [16, 18, 24, 200]);
        draw_text(
            &mut buf,
            w,
            hh,
            cx as i32 + 10,
            cy as i32 + 7,
            &status,
            CHIP_SCALE,
            [180, 190, 210, 255],
        );
        buf
    }
}

fn inside_rounded(r: Rect, radius: u32, x: u32, y: u32) -> bool {
    if x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h {
        return false;
    }
    let rad = radius.min(r.w / 2).min(r.h / 2) as f32;
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let cx = fx.clamp(r.x as f32 + rad, (r.x + r.w) as f32 - rad);
    let cy = fy.clamp(r.y as f32 + rad, (r.y + r.h) as f32 - rad);
    ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() <= rad
}

fn rounded_rect(buf: &mut [u8], w: usize, h: usize, r: Rect, radius: u32, rgba: [u8; 4]) {
    for y in r.y..(r.y + r.h).min(h as u32) {
        for x in r.x..(r.x + r.w).min(w as u32) {
            if inside_rounded(r, radius, x, y) {
                let i = (y as usize * w + x as usize) * 4;
                buf[i..i + 4].copy_from_slice(&rgba);
            }
        }
    }
}

fn rounded_outline(buf: &mut [u8], w: usize, h: usize, r: Rect, radius: u32, rgba: [u8; 4]) {
    for y in r.y..(r.y + r.h).min(h as u32) {
        for x in r.x..(r.x + r.w).min(w as u32) {
            let inside = inside_rounded(r, radius, x, y);
            let inner = Rect {
                x: r.x + 1,
                y: r.y + 1,
                w: r.w.saturating_sub(2),
                h: r.h.saturating_sub(2),
            };
            if inside && !inside_rounded(inner, radius.saturating_sub(1), x, y) {
                let i = (y as usize * w + x as usize) * 4;
                buf[i..i + 4].copy_from_slice(&rgba);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_is_compact_centred_and_hit_tests() {
        let bar = Bar::default();
        let rects = bar.layout(1920);
        assert_eq!(rects.len(), bar.buttons.len());
        for pair in rects.windows(2) {
            assert!(pair[0].x + pair[0].w <= pair[1].x, "buttons overlap");
        }
        let pill = bar.pill(1920);
        assert!(pill.w < 900, "pill too wide: {}", pill.w);
        assert!((pill.x as i32 - (1920 - pill.w) as i32 / 2).abs() <= 1, "centred");
        let r = rects[2];
        assert_eq!(bar.hit(1920, (r.x + 1) as f32, (r.y + 1) as f32), Some(2));
        assert_eq!(bar.hit(1920, (r.x + r.w + 1) as f32, (r.y + 1) as f32), None, "gap");
        assert_eq!(bar.hit(1920, 5.0, 5.0), None, "label row is not a button");
        assert_eq!(bar.buttons[2].action, BarAction::Arrange);
    }

    #[test]
    fn render_size_hover_label_and_status_chip() {
        let bar = Bar::default();
        let info = BarInfo {
            clock: "14:05  SAT 1 NOV".into(),
            windows: 3,
            rows: 1,
            distance_cm: 200,
            grabbed: false,
        };
        let plain = bar.render(1920, None, None, &info);
        assert_eq!(plain.len(), 1920 * bar.height() as usize * 4);
        // Outside pill and chip is transparent.
        assert_eq!(plain[(5 * 1920 + 5) * 4 + 3], 0);
        let hovered = bar.render(1920, Some(0), None, &info);
        // Hover paints a label in the top row above the first button.
        let r = bar.layout(1920)[0];
        let i = ((LABEL_H / 2) as usize * 1920 + (r.x + TILE / 2) as usize) * 4 + 3;
        assert_eq!(plain[i], 0);
        assert_ne!(hovered[i], 0);
        // Chip is painted near the right edge.
        let j = ((bar.pill(1920).y + 25) as usize * 1920 + 1900) * 4 + 3;
        assert_ne!(plain[j], 0);
    }
}
