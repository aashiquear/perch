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

use crate::{keymap, InputError, InputEvent, InputSink, InputSource};
use evdev::{uinput::VirtualDevice, AttributeSet, EventType, InputEvent as EvEvent, KeyCode, RelativeAxisCode};
use perch_proto::MouseButton;

/// Virtual keyboard + mouse via uinput.
pub struct UinputSink {
    dev: VirtualDevice,
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
        Ok(Self {
            dev,
            last_px: None,
            display_px,
        })
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
                // uinput has no absolute pointer without an ABS device that
                // most compositors treat as a tablet; relative deltas from the
                // last known position behave better. First event: jump from
                // the display centre.
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

/// Captures all keyboards and mice via evdev. Grabs them exclusively when
/// `set_grabbed(true)`.
pub struct EvdevSource {
    devices: Vec<evdev::Device>,
    grabbed: bool,
    pending: std::collections::VecDeque<InputEvent>,
    mods: u8,
}

impl EvdevSource {
    /// Open every device that looks like a keyboard or mouse (and is not our
    /// own virtual device).
    pub fn open_all() -> Result<Self, InputError> {
        let mut devices = Vec::new();
        for (_, d) in evdev::enumerate() {
            let name = d.name().unwrap_or("");
            if name.contains("Perch Virtual") {
                continue;
            }
            let keys = d.supported_keys();
            let is_kbd = keys.is_some_and(|k| k.contains(KeyCode::KEY_A));
            let is_mouse = keys.is_some_and(|k| k.contains(KeyCode::BTN_LEFT))
                && d.supported_relative_axes()
                    .is_some_and(|r| r.contains(RelativeAxisCode::REL_X));
            if is_kbd || is_mouse {
                tracing::info!(name, "capturing input device");
                devices.push(d);
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
            for d in &mut self.devices {
                if let Ok(events) = d.fetch_events() {
                    raw.extend(events);
                }
            }
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
        for d in &mut self.devices {
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
