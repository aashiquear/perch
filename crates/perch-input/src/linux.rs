//! Linux backend: inject through `/dev/uinput`, capture through evdev with
//! `EVIOCGRAB` so the local desktop stops seeing the physical devices while
//! input is routed to a remote peer (Barrier-style).
//!
//! Permissions: add the user to the `input` group and install
//! `udev/70-perch-uinput.rules` (makes `/dev/uinput` 0660 root:input).
//!
//! Wayland note: injection via uinput works on every compositor because it
//! is a kernel-level virtual device. Capture via evdev also works everywhere
//! but requires the `input` group. This is why we don't use XTest/libei here.

use crate::touchpad::{Raw, Touchpad, TouchpadConfig};
use crate::{keymap, AbsTarget, InputError, InputEvent, InputSink, InputSource};
use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent as EvEvent, KeyCode,
    RelativeAxisCode, UinputAbsSetup,
};
use perch_proto::MouseButton;

/// Range of the virtual absolute pointer; the compositor maps it onto the
/// whole desktop (all outputs), like a VM's USB tablet.
const ABS_MAX: i32 = 65535;

/// Virtual keyboard + mouse via uinput, plus a virtual absolute pointer
/// ("tablet") for placing the OS pointer at an exact desktop position.
pub struct UinputSink {
    dev: VirtualDevice,
    /// Absolute pointer device; `None` if it could not be created.
    abs: Option<VirtualDevice>,
    /// Where `MouseMoveAbs` lands; `None` → relative fallback.
    target: Option<AbsTarget>,
    /// Last absolute position we injected, in target display pixels, so we can
    /// convert `MouseMoveAbs` (window-normalised) to relative motion.
    last_px: Option<(f32, f32)>,
    /// Target display size in pixels for absolute → relative conversion.
    pub display_px: (f32, f32),
}

impl UinputSink {
    /// Create the virtual device ("Perch Virtual Input").
    pub fn new(display_px: (f32, f32)) -> Result<Self, InputError> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 1..=0x2ff {
            keys.insert(KeyCode(code));
        }
        for b in [
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::BTN_SIDE,
            KeyCode::BTN_EXTRA,
        ] {
            keys.insert(b);
        }
        let mut rel = AttributeSet::<RelativeAxisCode>::new();
        rel.insert(RelativeAxisCode::REL_X);
        rel.insert(RelativeAxisCode::REL_Y);
        rel.insert(RelativeAxisCode::REL_WHEEL);
        rel.insert(RelativeAxisCode::REL_HWHEEL);
        let dev = VirtualDevice::builder()
            .map_err(|e| InputError::Unavailable(format!("/dev/uinput: {e}")))?
            .name("Perch Virtual Input")
            .with_keys(&keys)
            .map_err(|e| InputError::Unavailable(e.to_string()))?
            .with_relative_axes(&rel)
            .map_err(|e| InputError::Unavailable(e.to_string()))?
            .build()
            .map_err(|e| InputError::Unavailable(format!("uinput build: {e}")))?;
        let abs = match Self::abs_device() {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::warn!("no absolute pointer device ({e}); MouseMoveAbs will use relative motion");
                None
            }
        };
        Ok(Self {
            dev,
            abs,
            target: None,
            last_px: None,
            display_px,
        })
    }

    fn abs_device() -> Result<VirtualDevice, InputError> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::BTN_LEFT);
        keys.insert(KeyCode::BTN_RIGHT);
        keys.insert(KeyCode::BTN_MIDDLE);
        let info = AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0);
        VirtualDevice::builder()
            .map_err(|e| InputError::Unavailable(format!("/dev/uinput: {e}")))?
            .name("Perch Virtual Pointer")
            .with_keys(&keys)
            .map_err(|e| InputError::Unavailable(e.to_string()))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, info))
            .map_err(|e| InputError::Unavailable(e.to_string()))?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, info))
            .map_err(|e| InputError::Unavailable(e.to_string()))?
            .build()
            .map_err(|e| InputError::Unavailable(format!("uinput abs build: {e}")))
    }

    fn key(&mut self, code: u16, down: bool) -> Result<(), InputError> {
        self.dev
            .emit(&[EvEvent::new(EventType::KEY.0, code, down as i32)])
            .map_err(InputError::Io)
    }
}

fn button_code(b: MouseButton) -> u16 {
    match b {
        MouseButton::Left => KeyCode::BTN_LEFT.0,
        MouseButton::Right => KeyCode::BTN_RIGHT.0,
        MouseButton::Middle => KeyCode::BTN_MIDDLE.0,
        MouseButton::Back => KeyCode::BTN_SIDE.0,
        MouseButton::Forward => KeyCode::BTN_EXTRA.0,
    }
}

impl InputSink for UinputSink {
    fn set_abs_target(&mut self, target: Option<AbsTarget>) {
        self.target = target;
    }

    fn inject(&mut self, ev: InputEvent) -> Result<(), InputError> {
        match ev {
            InputEvent::MouseMove { dx, dy } => self
                .dev
                .emit(&[
                    EvEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, dx.round() as i32),
                    EvEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, dy.round() as i32),
                ])
                .map_err(InputError::Io),
            InputEvent::MouseMoveAbs { x, y } => {
                if let (Some(abs), Some(t)) = (self.abs.as_mut(), self.target) {
                    // Desktop position in logical pixels → 0..ABS_MAX over the
                    // whole desktop.
                    let dx = t.rect.0 + x.clamp(0.0, 1.0) * t.rect.2;
                    let dy = t.rect.1 + y.clamp(0.0, 1.0) * t.rect.3;
                    let ax = (dx / t.bounds.0.max(1.0) * ABS_MAX as f32).round() as i32;
                    let ay = (dy / t.bounds.1.max(1.0) * ABS_MAX as f32).round() as i32;
                    return abs
                        .emit(&[
                            EvEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, ax.clamp(0, ABS_MAX)),
                            EvEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, ay.clamp(0, ABS_MAX)),
                        ])
                        .map_err(InputError::Io);
                }
                // No absolute device / unknown rectangle: relative deltas from
                // the last known position. First event: jump from the display
                // centre.
                let px = (x * self.display_px.0, y * self.display_px.1);
                let from = self
                    .last_px
                    .unwrap_or((self.display_px.0 / 2.0, self.display_px.1 / 2.0));
                self.last_px = Some(px);
                self.inject(InputEvent::MouseMove {
                    dx: px.0 - from.0,
                    dy: px.1 - from.1,
                })
            }
            InputEvent::MouseButton { button, down } if self.abs.is_some() && self.target.is_some() => {
                // Buttons on the absolute device so the click pairs with the
                // position it set.
                let code = button_code(button);
                if matches!(button, MouseButton::Left | MouseButton::Right | MouseButton::Middle) {
                    self.abs
                        .as_mut()
                        .expect("checked")
                        .emit(&[EvEvent::new(EventType::KEY.0, code, down as i32)])
                        .map_err(InputError::Io)
                } else {
                    self.key(code, down)
                }
            }
            InputEvent::MouseButton { button, down } => self.key(button_code(button), down),
            InputEvent::Scroll { dx, dy } => self
                .dev
                .emit(&[
                    EvEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_HWHEEL.0, dx.round() as i32),
                    EvEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, dy.round() as i32),
                ])
                .map_err(InputError::Io),
            InputEvent::Key { usage, down, .. } => match keymap::hid_to_evdev(usage) {
                Some(code) => self.key(code, down),
                None => {
                    tracing::debug!(usage, "unmapped HID usage");
                    Ok(())
                }
            },
            InputEvent::Text { ch } => {
                if let Some((usage, shift)) = keymap::char_to_usage_us(ch) {
                    let code = keymap::hid_to_evdev(usage).unwrap_or(0);
                    if shift {
                        self.key(42, true)?; // KEY_LEFTSHIFT
                    }
                    self.key(code, true)?;
                    self.key(code, false)?;
                    if shift {
                        self.key(42, false)?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// Captures all keyboards, mice and touchpads via evdev. Grabs them
/// exclusively when `set_grabbed(true)`.
pub struct EvdevSource {
    /// Each device with its touchpad interpreter when it is an absolute pad.
    devices: Vec<(evdev::Device, Option<Touchpad>)>,
    grabbed: bool,
    pending: std::collections::VecDeque<InputEvent>,
    mods: u8,
}

impl EvdevSource {
    /// Open every device that looks like a keyboard, mouse or touchpad (and
    /// is not our own virtual device).
    pub fn open_all() -> Result<Self, InputError> {
        let mut devices = Vec::new();
        for (_, d) in evdev::enumerate() {
            let name = d.name().unwrap_or("");
            if name.contains("Perch Virtual") {
                continue;
            }
            // Devices open blocking by default; the round-robin poll below
            // must never park on one device while another has events.
            if let Err(e) = d.set_nonblocking(true) {
                tracing::warn!(name, "evdev nonblocking: {e}");
                continue;
            }
            let keys = d.supported_keys();
            let is_kbd = keys.is_some_and(|k| k.contains(KeyCode::KEY_A));
            let has_rel = d
                .supported_relative_axes()
                .is_some_and(|r| r.contains(RelativeAxisCode::REL_X));
            let is_mouse = keys.is_some_and(|k| k.contains(KeyCode::BTN_LEFT)) && has_rel;
            let is_pad = !has_rel
                && keys.is_some_and(|k| k.contains(KeyCode::BTN_TOUCH) && k.contains(KeyCode::BTN_TOOL_FINGER))
                && d.supported_absolute_axes()
                    .is_some_and(|a| a.contains(AbsoluteAxisCode::ABS_X) && a.contains(AbsoluteAxisCode::ABS_Y));
            if is_pad {
                let pad = match d.get_absinfo() {
                    Ok(iter) => {
                        let mut cfg = TouchpadConfig::default();
                        let (mut rx, mut ry) = (0, 0);
                        for (axis, info) in iter {
                            if axis == AbsoluteAxisCode::ABS_X {
                                cfg.units_per_mm_x = info.resolution() as f32;
                                rx = info.maximum() - info.minimum();
                            } else if axis == AbsoluteAxisCode::ABS_Y {
                                cfg.units_per_mm_y = info.resolution() as f32;
                                ry = info.maximum() - info.minimum();
                            }
                        }
                        tracing::info!(name, rx, ry, upm = ?(cfg.units_per_mm_x, cfg.units_per_mm_y), "capturing touchpad");
                        Touchpad::new(cfg, rx, ry)
                    }
                    Err(e) => {
                        tracing::warn!(name, "touchpad absinfo: {e}");
                        continue;
                    }
                };
                devices.push((d, Some(pad)));
            } else if is_kbd || is_mouse {
                tracing::info!(name, "capturing input device");
                devices.push((d, None));
            }
        }
        if devices.is_empty() {
            return Err(InputError::Unavailable(
                "no evdev keyboards/mice readable — add yourself to the `input` group".into(),
            ));
        }
        Ok(Self {
            devices,
            grabbed: false,
            pending: Default::default(),
            mods: 0,
        })
    }

    /// Natural vs reverse two-finger scrolling on every captured touchpad.
    pub fn set_natural_scroll(&mut self, natural: bool) {
        for (_, pad) in &mut self.devices {
            if let Some(p) = pad {
                p.set_natural_scroll(natural);
            }
        }
    }

    fn translate(&mut self, ev: EvEvent) -> Option<InputEvent> {
        match ev.event_type() {
            EventType::KEY => {
                let code = ev.code();
                let down = ev.value() != 0;
                if ev.value() == 2 {
                    return None; // autorepeat; the sink's OS will repeat itself
                }
                match code {
                    c if c == KeyCode::BTN_LEFT.0 => Some(InputEvent::MouseButton {
                        button: MouseButton::Left,
                        down,
                    }),
                    c if c == KeyCode::BTN_RIGHT.0 => Some(InputEvent::MouseButton {
                        button: MouseButton::Right,
                        down,
                    }),
                    c if c == KeyCode::BTN_MIDDLE.0 => Some(InputEvent::MouseButton {
                        button: MouseButton::Middle,
                        down,
                    }),
                    c if c == KeyCode::BTN_SIDE.0 => Some(InputEvent::MouseButton {
                        button: MouseButton::Back,
                        down,
                    }),
                    c if c == KeyCode::BTN_EXTRA.0 => Some(InputEvent::MouseButton {
                        button: MouseButton::Forward,
                        down,
                    }),
                    _ => {
                        let usage = keymap::evdev_to_hid(code)?;
                        let bit = match usage {
                            0xe0 | 0xe4 => 1,
                            0xe1 | 0xe5 => 2,
                            0xe2 | 0xe6 => 4,
                            0xe3 | 0xe7 => 8,
                            _ => 0,
                        };
                        if down {
                            self.mods |= bit;
                        } else {
                            self.mods &= !bit;
                        }
                        Some(InputEvent::Key {
                            usage,
                            down,
                            mods: self.mods,
                        })
                    }
                }
            }
            EventType::RELATIVE => match RelativeAxisCode(ev.code()) {
                RelativeAxisCode::REL_X => Some(InputEvent::MouseMove {
                    dx: ev.value() as f32,
                    dy: 0.0,
                }),
                RelativeAxisCode::REL_Y => Some(InputEvent::MouseMove {
                    dx: 0.0,
                    dy: ev.value() as f32,
                }),
                RelativeAxisCode::REL_WHEEL => Some(InputEvent::Scroll {
                    dx: 0.0,
                    dy: ev.value() as f32,
                }),
                RelativeAxisCode::REL_HWHEEL => Some(InputEvent::Scroll {
                    dx: ev.value() as f32,
                    dy: 0.0,
                }),
                _ => None,
            },
            _ => None,
        }
    }
}

impl InputSource for EvdevSource {
    fn next(&mut self) -> Result<InputEvent, InputError> {
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Ok(e);
            }
            // Simple round-robin poll; the daemon runs this on its own thread.
            // (A production version should use epoll over all fds.)
            let mut raw: Vec<EvEvent> = Vec::new();
            let mut touch: Vec<InputEvent> = Vec::new();
            for (d, pad) in &mut self.devices {
                let Ok(events) = d.fetch_events() else { continue };
                match pad {
                    None => raw.extend(events),
                    Some(pad) => {
                        for ev in events {
                            let r = match ev.event_type() {
                                EventType::ABSOLUTE => Raw::Abs {
                                    code: ev.code(),
                                    value: ev.value(),
                                },
                                EventType::KEY => Raw::Key {
                                    code: ev.code(),
                                    down: ev.value() != 0,
                                },
                                EventType::SYNCHRONIZATION if ev.code() == 0 => {
                                    let t_us = ev
                                        .timestamp()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_micros() as u64)
                                        .unwrap_or(0);
                                    Raw::Sync { t_us }
                                }
                                _ => continue,
                            };
                            pad.feed(r, &mut touch);
                        }
                    }
                }
            }
            self.pending.extend(touch);
            for ev in raw {
                if let Some(t) = self.translate(ev) {
                    self.pending.push_back(t);
                }
            }
            if self.pending.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }

    fn set_grabbed(&mut self, grabbed: bool) -> Result<(), InputError> {
        if grabbed == self.grabbed {
            return Ok(());
        }
        for (d, _) in &mut self.devices {
            if grabbed {
                d.grab()?;
            } else {
                d.ungrab()?;
            }
        }
        self.grabbed = grabbed;
        Ok(())
    }
}
