//! Procedural 24×24 icons for the control bar (no image assets: a few
//! lines, rectangles and circles rasterised into a coverage mask).

/// Icon size in pixels.
pub const ICON: usize = 24;

/// Which icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Crosshair.
    Recenter,
    /// Target with a dot.
    CentreHere,
    /// 2×2 grid.
    Arrange,
    /// Three bars with a minus.
    RowsMinus,
    /// Three bars with a plus.
    RowsPlus,
    /// Chevrons pointing in.
    Nearer,
    /// Chevrons pointing out.
    Farther,
    /// Window with a plus.
    Pick,
    /// Gyro: circle, dot and a tilted ellipse-ish arc.
    Calibrate,
    /// Box with an X.
    Clear,
    /// Pointer arrow.
    Grab,
    /// Laptop with an arrow pointing into it.
    ToLaptop,
    /// Window with a cross: close the app.
    CloseApp,
    /// Horizontal double arrow: resize the display.
    Resize,
}

/// 24×24 coverage mask (0 or 255 per cell).
pub type Mask = [[u8; ICON]; ICON];

fn put(m: &mut Mask, x: i32, y: i32) {
    if (0..ICON as i32).contains(&x) && (0..ICON as i32).contains(&y) {
        m[y as usize][x as usize] = 255;
    }
}

fn line(m: &mut Mask, x0: i32, y0: i32, x1: i32, y1: i32) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        put(m, x, y);
        put(m, x + 1, y); // 2-px stroke
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn rect(m: &mut Mask, x0: i32, y0: i32, x1: i32, y1: i32, fill: bool) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            if fill
                || x == x0
                || x == x1
                || y == y0
                || y == y1
                || x == x0 + 1
                || x == x1 - 1
                || y == y0 + 1
                || y == y1 - 1
            {
                put(m, x, y);
            }
        }
    }
}

fn circle(m: &mut Mask, cx: f32, cy: f32, r: f32, fill: bool) {
    for y in 0..ICON as i32 {
        for x in 0..ICON as i32 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if (fill && d <= r) || (!fill && (d - r).abs() < 1.1) {
                put(m, x, y);
            }
        }
    }
}

/// Rasterise an icon.
pub fn mask(icon: Icon) -> Mask {
    let mut m: Mask = [[0; ICON]; ICON];
    match icon {
        Icon::Recenter => {
            circle(&mut m, 12.0, 12.0, 7.0, false);
            line(&mut m, 12, 1, 12, 6);
            line(&mut m, 12, 17, 12, 22);
            line(&mut m, 1, 12, 6, 12);
            line(&mut m, 17, 12, 22, 12);
        }
        Icon::CentreHere => {
            circle(&mut m, 12.0, 12.0, 9.0, false);
            circle(&mut m, 12.0, 12.0, 3.0, true);
        }
        Icon::Arrange => {
            rect(&mut m, 2, 2, 10, 10, false);
            rect(&mut m, 13, 2, 21, 10, false);
            rect(&mut m, 2, 13, 10, 21, false);
            rect(&mut m, 13, 13, 21, 21, false);
        }
        Icon::RowsMinus | Icon::RowsPlus => {
            for y in [4, 11, 18] {
                rect(&mut m, 2, y, 14, y + 1, true);
            }
            rect(&mut m, 17, 11, 22, 12, true);
            if icon == Icon::RowsPlus {
                rect(&mut m, 19, 9, 20, 14, true);
            }
        }
        Icon::Nearer => {
            line(&mut m, 2, 4, 9, 12);
            line(&mut m, 9, 12, 2, 20);
            line(&mut m, 21, 4, 14, 12);
            line(&mut m, 14, 12, 21, 20);
        }
        Icon::Farther => {
            line(&mut m, 9, 4, 2, 12);
            line(&mut m, 2, 12, 9, 20);
            line(&mut m, 14, 4, 21, 12);
            line(&mut m, 21, 12, 14, 20);
        }
        Icon::Pick => {
            rect(&mut m, 2, 4, 21, 20, false);
            rect(&mut m, 2, 4, 21, 7, true);
            rect(&mut m, 11, 10, 12, 18, true);
            rect(&mut m, 8, 13, 15, 14, true);
        }
        Icon::Calibrate => {
            circle(&mut m, 12.0, 12.0, 9.0, false);
            circle(&mut m, 12.0, 12.0, 2.0, true);
            line(&mut m, 4, 16, 20, 8);
        }
        Icon::Clear => {
            rect(&mut m, 2, 2, 21, 21, false);
            line(&mut m, 7, 7, 16, 16);
            line(&mut m, 16, 7, 7, 16);
        }
        Icon::Grab => {
            // Arrow pointer.
            line(&mut m, 4, 2, 4, 19);
            line(&mut m, 4, 2, 17, 13);
            line(&mut m, 17, 13, 11, 14);
            line(&mut m, 11, 14, 15, 22);
            line(&mut m, 4, 19, 8, 15);
            line(&mut m, 8, 15, 15, 22);
        }
        Icon::ToLaptop => {
            rect(&mut m, 5, 11, 18, 18, false);
            line(&mut m, 2, 21, 21, 21);
            line(&mut m, 20, 2, 12, 10);
            line(&mut m, 12, 10, 12, 5);
            line(&mut m, 12, 10, 17, 10);
        }
        Icon::CloseApp => {
            rect(&mut m, 2, 3, 21, 20, false);
            line(&mut m, 2, 7, 21, 7);
            line(&mut m, 7, 10, 16, 18);
            line(&mut m, 16, 10, 7, 18);
        }
        Icon::Resize => {
            line(&mut m, 2, 12, 21, 12);
            line(&mut m, 2, 12, 6, 8);
            line(&mut m, 2, 12, 6, 16);
            line(&mut m, 21, 12, 17, 8);
            line(&mut m, 21, 12, 17, 16);
            rect(&mut m, 8, 4, 15, 20, false);
        }
    }
    m
}

/// Blit a mask into an RGBA buffer at (`x`, `y`) with colour `rgba`
/// (alpha from the mask).
pub fn blit(buf: &mut [u8], buf_w: usize, buf_h: usize, x: i32, y: i32, m: &Mask, rgba: [u8; 4]) {
    for (iy, row) in m.iter().enumerate() {
        for (ix, a) in row.iter().enumerate() {
            if *a == 0 {
                continue;
            }
            let (px, py) = (x + ix as i32, y + iy as i32);
            if px < 0 || py < 0 || px as usize >= buf_w || py as usize >= buf_h {
                continue;
            }
            let i = (py as usize * buf_w + px as usize) * 4;
            buf[i..i + 4].copy_from_slice(&rgba);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws_something_inside_the_box() {
        for icon in [
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
        ] {
            let m = mask(icon);
            let lit: usize = m.iter().flatten().filter(|a| **a > 0).count();
            assert!(lit > 20 && lit < ICON * ICON / 2, "{icon:?}: {lit} px");
        }
        assert_ne!(mask(Icon::RowsMinus), mask(Icon::RowsPlus));
    }
}
