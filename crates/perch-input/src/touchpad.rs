//! Pure touchpad gesture interpreter: turns raw absolute-touch reports
//! (evdev-style codes, but no evdev dependency) into pointer motion, taps
//! and two-finger scroll. Used by the Linux evdev source for laptops whose
//! touchpad exposes no relative axes (e.g. Apple internal trackpads).
//!
//! Only the single-touch position (`ABS_X`/`ABS_Y`), the finger-count tool
//! buttons and `BTN_TOUCH`/`BTN_LEFT` are needed; multitouch slots are
//! ignored, which is enough for move / tap / click / two-finger scroll.

use perch_proto::{InputEvent, MouseButton};

/// `ABS_X`.
pub const ABS_X: u16 = 0x00;
/// `ABS_Y`.
pub const ABS_Y: u16 = 0x01;
/// `BTN_LEFT` (physical click).
pub const BTN_LEFT: u16 = 0x110;
/// `BTN_TOOL_FINGER` (one finger).
pub const BTN_TOOL_FINGER: u16 = 0x145;
/// `BTN_TOUCH`.
pub const BTN_TOUCH: u16 = 0x14a;
/// `BTN_TOOL_DOUBLETAP` (two fingers).
pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
/// `BTN_TOOL_TRIPLETAP` (three fingers).
pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
/// `BTN_TOOL_QUADTAP` (four fingers).
pub const BTN_TOOL_QUADTAP: u16 = 0x14f;

/// Raw report from the device; one `Sync` closes a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Raw {
    /// Absolute axis value.
    Abs {
        /// Axis code (`ABS_X` / `ABS_Y`; others ignored).
        code: u16,
        /// Value in device units.
        value: i32,
    },
    /// Button / tool state.
    Key {
        /// Key code.
        code: u16,
        /// True on press.
        down: bool,
    },
    /// `SYN_REPORT` with the frame time in microseconds.
    Sync {
        /// Timestamp.
        t_us: u64,
    },
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchpadConfig {
    /// Device units per millimetre on X (from `ABS_X` resolution; 0 → guess).
    pub units_per_mm_x: f32,
    /// Device units per millimetre on Y.
    pub units_per_mm_y: f32,
    /// Base pointer speed in pixels per millimetre of finger travel.
    pub speed_px_per_mm: f32,
    /// Enable tap-to-click (one finger → left, two → right).
    pub tap_to_click: bool,
    /// Natural ("content follows finger") two-finger scrolling.
    pub natural_scroll: bool,
    /// Millimetres of finger travel per scroll line.
    pub mm_per_scroll_line: f32,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            units_per_mm_x: 0.0,
            units_per_mm_y: 0.0,
            speed_px_per_mm: 9.0,
            tap_to_click: true,
            natural_scroll: true,
            mm_per_scroll_line: 3.0,
        }
    }
}

/// Maximum tap duration.
const TAP_MAX_US: u64 = 180_000;
/// Maximum finger travel for a tap.
const TAP_MAX_MM: f32 = 2.0;

/// Gesture state machine for one touchpad.
#[derive(Debug)]
pub struct Touchpad {
    cfg: TouchpadConfig,
    upm: (f32, f32),
    x: i32,
    y: i32,
    touching: bool,
    fingers: u8,
    /// Position at the last frame (device units), valid for delta.
    last: Option<(i32, i32)>,
    /// Finger count at the last frame.
    last_fingers: u8,
    /// Tap candidate: start time, start position, max fingers, travel.
    tap: Option<Tap>,
    /// Physical button held: which logical button was pressed.
    phys: Option<MouseButton>,
    /// Fractional scroll carry.
    scroll_acc: (f32, f32),
    /// Pending key state changes to apply on sync.
    pending_touch: Option<bool>,
    pending_phys: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct Tap {
    t0: u64,
    x0: i32,
    y0: i32,
    max_fingers: u8,
    moved: bool,
}

impl Touchpad {
    /// New interpreter. `units_per_mm` of 0 falls back to a 100 mm-wide pad
    /// assumption via `range_x`.
    pub fn new(cfg: TouchpadConfig, range_x: i32, range_y: i32) -> Self {
        let ux = if cfg.units_per_mm_x > 0.0 {
            cfg.units_per_mm_x
        } else {
            (range_x.max(1) as f32 / 100.0).max(1.0)
        };
        let uy = if cfg.units_per_mm_y > 0.0 {
            cfg.units_per_mm_y
        } else {
            (range_y.max(1) as f32 / 60.0).max(1.0)
        };
        Self {
            cfg,
            upm: (ux, uy),
            x: 0,
            y: 0,
            touching: false,
            fingers: 0,
            last: None,
            last_fingers: 0,
            tap: None,
            phys: None,
            scroll_acc: (0.0, 0.0),
            pending_touch: None,
            pending_phys: None,
        }
    }

    /// Feed one raw report; events produced at `Sync` are pushed to `out`.
    pub fn feed(&mut self, raw: Raw, out: &mut Vec<InputEvent>) {
        match raw {
            Raw::Abs { code: ABS_X, value } => self.x = value,
            Raw::Abs { code: ABS_Y, value } => self.y = value,
            Raw::Abs { .. } => {}
            Raw::Key { code, down } => match code {
                BTN_TOUCH => self.pending_touch = Some(down),
                BTN_LEFT => self.pending_phys = Some(down),
                BTN_TOOL_FINGER if down => self.fingers = 1,
                BTN_TOOL_DOUBLETAP if down => self.fingers = 2,
                BTN_TOOL_TRIPLETAP if down => self.fingers = 3,
                BTN_TOOL_QUADTAP if down => self.fingers = 4,
                BTN_TOOL_FINGER | BTN_TOOL_DOUBLETAP | BTN_TOOL_TRIPLETAP | BTN_TOOL_QUADTAP => {
                    // Tool release: the new count arrives as another tool press
                    // in the same frame, or none when all fingers lift.
                    let n = match code {
                        BTN_TOOL_FINGER => 1,
                        BTN_TOOL_DOUBLETAP => 2,
                        BTN_TOOL_TRIPLETAP => 3,
                        _ => 4,
                    };
                    if self.fingers == n {
                        self.fingers = 0;
                    }
                }
                _ => {}
            },
            Raw::Sync { t_us } => self.sync(t_us, out),
        }
    }

    fn mm(&self, dx: i32, dy: i32) -> (f32, f32) {
        (dx as f32 / self.upm.0, dy as f32 / self.upm.1)
    }

    fn sync(&mut self, t_us: u64, out: &mut Vec<InputEvent>) {
        // Physical click: two fingers resting → right button.
        if let Some(down) = self.pending_phys.take() {
            if down && self.phys.is_none() {
                let b = if self.fingers >= 2 {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                self.phys = Some(b);
                out.push(InputEvent::MouseButton { button: b, down: true });
                self.tap = None;
            } else if !down {
                if let Some(b) = self.phys.take() {
                    out.push(InputEvent::MouseButton { button: b, down: false });
                }
            }
        }
        if let Some(t) = self.pending_touch.take() {
            if t && !self.touching {
                self.touching = true;
                self.last = None;
                self.tap = Some(Tap {
                    t0: t_us,
                    x0: self.x,
                    y0: self.y,
                    max_fingers: self.fingers.max(1),
                    moved: false,
                });
            } else if !t && self.touching {
                self.touching = false;
                self.last = None;
                if let Some(tap) = self.tap.take() {
                    let quick = t_us.saturating_sub(tap.t0) <= TAP_MAX_US;
                    if self.cfg.tap_to_click && quick && !tap.moved && self.phys.is_none() {
                        let b = match tap.max_fingers {
                            1 => Some(MouseButton::Left),
                            2 => Some(MouseButton::Right),
                            3 => Some(MouseButton::Middle),
                            _ => None,
                        };
                        if let Some(b) = b {
                            out.push(InputEvent::MouseButton { button: b, down: true });
                            out.push(InputEvent::MouseButton { button: b, down: false });
                        }
                    }
                }
                self.last_fingers = 0;
                return;
            }
        }
        if !self.touching {
            return;
        }
        let fingers = self.fingers.max(1);
        if let Some(tap) = self.tap {
            let (mx, my) = self.mm(self.x - tap.x0, self.y - tap.y0);
            let moved = tap.moved || (mx * mx + my * my).sqrt() > TAP_MAX_MM;
            self.tap = Some(Tap {
                max_fingers: tap.max_fingers.max(fingers),
                moved,
                ..tap
            });
        }
        if fingers != self.last_fingers {
            // Finger count changed: don't produce a jump from the new centroid.
            self.last_fingers = fingers;
            self.last = Some((self.x, self.y));
            self.scroll_acc = (0.0, 0.0);
            return;
        }
        let Some((lx, ly)) = self.last else {
            self.last = Some((self.x, self.y));
            return;
        };
        let (dx_mm, dy_mm) = self.mm(self.x - lx, self.y - ly);
        self.last = Some((self.x, self.y));
        if dx_mm == 0.0 && dy_mm == 0.0 {
            return;
        }
        match fingers {
            1 => {
                // Mild acceleration: slow strokes are precise, fast ones travel.
                let d = (dx_mm * dx_mm + dy_mm * dy_mm).sqrt();
                let gain = self.cfg.speed_px_per_mm * (0.7 + 1.2 * d).clamp(0.7, 2.5);
                out.push(InputEvent::MouseMove {
                    dx: dx_mm * gain,
                    dy: dy_mm * gain,
                });
            }
            2 => {
                let sign = if self.cfg.natural_scroll { 1.0 } else { -1.0 };
                let lines = self.cfg.mm_per_scroll_line.max(0.1);
                self.scroll_acc.0 += -sign * dx_mm / lines;
                self.scroll_acc.1 += sign * dy_mm / lines;
                let sx = self.scroll_acc.0.trunc();
                let sy = self.scroll_acc.1.trunc();
                if sx != 0.0 || sy != 0.0 {
                    self.scroll_acc.0 -= sx;
                    self.scroll_acc.1 -= sy;
                    out.push(InputEvent::Scroll { dx: sx, dy: sy });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> Touchpad {
        Touchpad::new(
            TouchpadConfig {
                units_per_mm_x: 10.0,
                units_per_mm_y: 10.0,
                ..Default::default()
            },
            1000,
            600,
        )
    }

    fn frame(p: &mut Touchpad, out: &mut Vec<InputEvent>, raws: &[Raw], t_us: u64) {
        for r in raws {
            p.feed(*r, out);
        }
        p.feed(Raw::Sync { t_us }, out);
    }

    fn touch(x: i32, y: i32, down: bool) -> Vec<Raw> {
        vec![
            Raw::Key { code: BTN_TOUCH, down },
            Raw::Key {
                code: BTN_TOOL_FINGER,
                down,
            },
            Raw::Abs { code: ABS_X, value: x },
            Raw::Abs { code: ABS_Y, value: y },
        ]
    }

    #[test]
    fn one_finger_drag_moves_pointer_in_finger_direction() {
        let mut p = pad();
        let mut out = Vec::new();
        frame(&mut p, &mut out, &touch(100, 100, true), 0);
        assert!(out.is_empty(), "touch-down alone must not move");
        frame(
            &mut p,
            &mut out,
            &[
                Raw::Abs {
                    code: ABS_X,
                    value: 120,
                },
                Raw::Abs { code: ABS_Y, value: 90 },
            ],
            10_000,
        );
        match out.as_slice() {
            [InputEvent::MouseMove { dx, dy }] => {
                assert!(*dx > 0.0 && *dy < 0.0, "{out:?}");
            }
            _ => panic!("{out:?}"),
        }
    }

    #[test]
    fn quick_still_tap_clicks_long_or_moving_touch_does_not() {
        let mut p = pad();
        let mut out = Vec::new();
        frame(&mut p, &mut out, &touch(100, 100, true), 0);
        frame(&mut p, &mut out, &touch(100, 100, false), 50_000);
        assert_eq!(
            out,
            vec![
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    down: true
                },
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    down: false
                },
            ]
        );
        out.clear();
        frame(&mut p, &mut out, &touch(100, 100, true), 100_000);
        frame(&mut p, &mut out, &touch(100, 100, false), 600_000);
        assert!(out.is_empty(), "slow press is not a tap: {out:?}");
        frame(&mut p, &mut out, &touch(100, 100, true), 700_000);
        frame(
            &mut p,
            &mut out,
            &[Raw::Abs {
                code: ABS_X,
                value: 200,
            }],
            720_000,
        );
        frame(&mut p, &mut out, &touch(200, 100, false), 740_000);
        assert!(
            !out.iter().any(|e| matches!(e, InputEvent::MouseButton { .. })),
            "{out:?}"
        );
    }

    #[test]
    fn two_finger_drag_scrolls_naturally_and_not_pointer() {
        let mut p = pad();
        let mut out = Vec::new();
        frame(
            &mut p,
            &mut out,
            &[
                Raw::Key {
                    code: BTN_TOUCH,
                    down: true,
                },
                Raw::Key {
                    code: BTN_TOOL_DOUBLETAP,
                    down: true,
                },
                Raw::Abs {
                    code: ABS_X,
                    value: 100,
                },
                Raw::Abs {
                    code: ABS_Y,
                    value: 100,
                },
            ],
            0,
        );
        // Fingers move down 60 mm → 20 lines, content follows (wheel "up").
        frame(
            &mut p,
            &mut out,
            &[Raw::Abs {
                code: ABS_Y,
                value: 700,
            }],
            10_000,
        );
        assert_eq!(out, vec![InputEvent::Scroll { dx: 0.0, dy: 20.0 }]);
        out.clear();
        frame(
            &mut p,
            &mut out,
            &[
                Raw::Key {
                    code: BTN_TOUCH,
                    down: false,
                },
                Raw::Key {
                    code: BTN_TOOL_DOUBLETAP,
                    down: false,
                },
            ],
            400_000,
        );
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn physical_click_with_two_fingers_is_right_button() {
        let mut p = pad();
        let mut out = Vec::new();
        frame(
            &mut p,
            &mut out,
            &[
                Raw::Key {
                    code: BTN_TOUCH,
                    down: true,
                },
                Raw::Key {
                    code: BTN_TOOL_DOUBLETAP,
                    down: true,
                },
                Raw::Key {
                    code: BTN_LEFT,
                    down: true,
                },
            ],
            0,
        );
        frame(
            &mut p,
            &mut out,
            &[Raw::Key {
                code: BTN_LEFT,
                down: false,
            }],
            300_000,
        );
        frame(
            &mut p,
            &mut out,
            &[Raw::Key {
                code: BTN_TOUCH,
                down: false,
            }],
            320_000,
        );
        assert_eq!(
            out,
            vec![
                InputEvent::MouseButton {
                    button: MouseButton::Right,
                    down: true
                },
                InputEvent::MouseButton {
                    button: MouseButton::Right,
                    down: false
                },
            ]
        );
    }

    #[test]
    fn finger_count_change_does_not_jump() {
        let mut p = pad();
        let mut out = Vec::new();
        frame(&mut p, &mut out, &touch(100, 100, true), 0);
        frame(
            &mut p,
            &mut out,
            &[Raw::Abs {
                code: ABS_X,
                value: 110,
            }],
            10_000,
        );
        out.clear();
        // Second finger lands far away: centroid jumps, pointer must not.
        frame(
            &mut p,
            &mut out,
            &[
                Raw::Key {
                    code: BTN_TOOL_FINGER,
                    down: false,
                },
                Raw::Key {
                    code: BTN_TOOL_DOUBLETAP,
                    down: true,
                },
                Raw::Abs {
                    code: ABS_X,
                    value: 600,
                },
            ],
            20_000,
        );
        assert!(out.is_empty(), "{out:?}");
    }
}
