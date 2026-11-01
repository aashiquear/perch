//! The control bar: a strip of buttons pinned to the bottom of the glasses
//! panel. Pure: layout and hit-testing are integer maths, rendering writes
//! an RGBA buffer the compositor uploads as a layer.

use crate::font::{draw_text, text_width};

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
}

/// A button's label and action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    /// Text on the button.
    pub label: &'static str,
    /// What it does.
    pub action: BarAction,
}

/// Rectangle in panel pixels.
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

/// Pixel scale of the bitmap font on the bar.
const TEXT_SCALE: usize = 3;
const PAD_X: u32 = 18;
const PAD_Y: u32 = 12;
const SPACING: u32 = 10;
const BAR_PAD: u32 = 10;

/// The bar.
#[derive(Debug, Clone)]
pub struct Bar {
    /// Buttons, left to right.
    pub buttons: Vec<Button>,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            buttons: vec![
                Button {
                    label: "RECENTER",
                    action: BarAction::Recenter,
                },
                Button {
                    label: "CENTRE HERE",
                    action: BarAction::CentreHere,
                },
                Button {
                    label: "ARRANGE",
                    action: BarAction::Arrange,
                },
                Button {
                    label: "ROWS -",
                    action: BarAction::RowsMinus,
                },
                Button {
                    label: "ROWS +",
                    action: BarAction::RowsPlus,
                },
                Button {
                    label: "NEARER",
                    action: BarAction::Nearer,
                },
                Button {
                    label: "FARTHER",
                    action: BarAction::Farther,
                },
                Button {
                    label: "PICK",
                    action: BarAction::Pick,
                },
                Button {
                    label: "CALIBRATE",
                    action: BarAction::Calibrate,
                },
                Button {
                    label: "CLEAR",
                    action: BarAction::Clear,
                },
            ],
        }
    }
}

impl Bar {
    /// Bar height in pixels.
    pub fn height(&self) -> u32 {
        (crate::font::GLYPH_H * TEXT_SCALE) as u32 + 2 * PAD_Y + 2 * BAR_PAD
    }

    /// Button rectangles for a bar `panel_w` wide, centred.
    pub fn layout(&self, panel_w: u32) -> Vec<Rect> {
        let widths: Vec<u32> = self
            .buttons
            .iter()
            .map(|b| text_width(b.label, TEXT_SCALE) as u32 + 2 * PAD_X)
            .collect();
        let total: u32 = widths.iter().sum::<u32>() + SPACING * widths.len().saturating_sub(1) as u32;
        let mut x = panel_w.saturating_sub(total) / 2;
        let h = (crate::font::GLYPH_H * TEXT_SCALE) as u32 + 2 * PAD_Y;
        widths
            .iter()
            .map(|w| {
                let r = Rect {
                    x,
                    y: BAR_PAD,
                    w: *w,
                    h,
                };
                x += w + SPACING;
                r
            })
            .collect()
    }

    /// Which button is under (`px`, `py`), in bar-local pixels?
    pub fn hit(&self, panel_w: u32, px: f32, py: f32) -> Option<usize> {
        self.layout(panel_w).iter().position(|r| r.contains(px, py))
    }

    /// Render the bar as RGBA (`panel_w` × `height()`), with optional
    /// hovered / pressed button indices highlighted and a status line
    /// (e.g. "3 ROWS  2.0 M") on the right.
    pub fn render(&self, panel_w: u32, hover: Option<usize>, pressed: Option<usize>, status: &str) -> Vec<u8> {
        let h = self.height();
        let (w, hh) = (panel_w as usize, h as usize);
        let mut buf = vec![0u8; w * hh * 4];
        // Translucent dark strip.
        for px in buf.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[12, 12, 16, 170]);
        }
        for (i, (b, r)) in self.buttons.iter().zip(self.layout(panel_w)).enumerate() {
            let fill: [u8; 4] = if pressed == Some(i) {
                [90, 160, 255, 255]
            } else if hover == Some(i) {
                [70, 80, 100, 255]
            } else {
                [40, 44, 52, 255]
            };
            fill_rect(&mut buf, w, hh, r, fill);
            let text_w = text_width(b.label, TEXT_SCALE) as u32;
            let tx = r.x + (r.w - text_w) / 2;
            let ty = r.y + PAD_Y;
            draw_text(
                &mut buf,
                w,
                hh,
                tx as i32,
                ty as i32,
                b.label,
                TEXT_SCALE,
                [235, 235, 240, 255],
            );
        }
        if !status.is_empty() {
            let tw = text_width(status, 2) as i32;
            draw_text(
                &mut buf,
                w,
                hh,
                panel_w as i32 - tw - 12,
                (h / 2) as i32 - (crate::font::GLYPH_H as i32),
                status,
                2,
                [170, 180, 200, 255],
            );
        }
        buf
    }
}

fn fill_rect(buf: &mut [u8], w: usize, h: usize, r: Rect, rgba: [u8; 4]) {
    for y in r.y..(r.y + r.h).min(h as u32) {
        for x in r.x..(r.x + r.w).min(w as u32) {
            let i = (y as usize * w + x as usize) * 4;
            buf[i..i + 4].copy_from_slice(&rgba);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_fit_in_1080p_do_not_overlap_and_hit_test() {
        let bar = Bar::default();
        let rects = bar.layout(1920);
        assert_eq!(rects.len(), bar.buttons.len());
        for pair in rects.windows(2) {
            assert!(pair[0].x + pair[0].w <= pair[1].x, "buttons overlap");
        }
        let last = rects.last().unwrap();
        assert!(last.x + last.w <= 1920, "bar wider than the panel");
        assert!(rects[0].x > 0, "centred");
        let r = rects[2];
        assert_eq!(bar.hit(1920, (r.x + 1) as f32, (r.y + 1) as f32), Some(2));
        assert_eq!(
            bar.hit(1920, (r.x + r.w) as f32, (r.y + 1) as f32),
            None,
            "gap between buttons"
        );
        assert_eq!(bar.buttons[2].action, BarAction::Arrange);
    }

    #[test]
    fn render_has_the_right_size_and_highlights_hover() {
        let bar = Bar::default();
        let plain = bar.render(1920, None, None, "1 ROWS 2.0 M");
        assert_eq!(plain.len(), 1920 * bar.height() as usize * 4);
        let hovered = bar.render(1920, Some(0), None, "");
        let r = bar.layout(1920)[0];
        let i = ((r.y + 2) as usize * 1920 + (r.x + 2) as usize) * 4;
        assert_ne!(plain[i..i + 3], hovered[i..i + 3]);
    }
}
