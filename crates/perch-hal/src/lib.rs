//! `perch-hal` — glasses hardware abstraction.
//!
//! One trait, [`Glasses`], with two backends:
//!
//! * [`rokid::RokidGlasses`] (feature `rokid`) — wraps the MIT-licensed
//!   [`ar-drivers`](https://docs.rs/ar-drivers) crate, which talks to the
//!   Rokid Air/Max over libusb (VID `0x04d2`, PID `0x162f`, interrupt-in
//!   endpoint `0x82`). On Linux you need the udev rule in `udev/`.
//! * [`mock::MockGlasses`] (feature `mock`) — replays a recorded IMU log so
//!   the whole stack runs on CI and on machines without glasses. Recordings
//!   are made with `perch record`.
//!
//! The daemon chooses a backend with [`open_any`].

#![warn(missing_docs)]

use perch_proto::DisplayMode;
use perch_tracking::ImuSample;
use serde::{Deserialize, Serialize};

/// Errors from the HAL.
#[derive(Debug, thiserror::Error)]
pub enum HalError {
    /// No supported glasses are plugged in.
    #[error("no supported glasses found")]
    NotFound,
    /// Permission denied opening the USB device (missing udev rule / Android
    /// permission grant).
    #[error("permission denied opening glasses: {0}")]
    Permission(String),
    /// Glasses were unplugged or stopped responding.
    #[error("glasses disconnected: {0}")]
    Disconnected(String),
    /// The device does not support the requested operation.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    /// Any other backend error.
    #[error("{0}")]
    Other(String),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, HalError>;

/// Static description of a connected device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Human name, e.g. "Rokid Max".
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Horizontal FOV in radians.
    pub fov_h: f32,
    /// Display tilt relative to IMU, radians.
    pub display_tilt: f32,
    /// Display pipeline delay in microseconds.
    pub display_delay_us: u64,
    /// Whether the device has a magnetometer.
    pub has_mag: bool,
    /// Nominal IMU rate in Hz.
    pub imu_rate_hz: u32,
}

/// Events from the glasses beyond IMU samples.
#[derive(Debug, Clone, PartialEq)]
pub enum HalEvent {
    /// An IMU sample.
    Imu(ImuSample),
    /// A physical button on the temple was pressed. 0 = first button.
    Button(u8),
    /// Proximity sensor: glasses put on (`true`) / taken off (`false`).
    Worn(bool),
    /// Ambient light (vendor units).
    AmbientLight(u16),
    /// Display v-sync tick (for latency measurement).
    VSync,
}

/// Something that behaves like a pair of XR glasses.
pub trait Glasses: Send {
    /// Static info.
    fn info(&self) -> &DeviceInfo;
    /// Block until the next event. Returns `Disconnected` when unplugged.
    fn next_event(&mut self) -> Result<HalEvent>;
    /// Current display mode.
    fn display_mode(&mut self) -> Result<DisplayMode>;
    /// Change display mode. Note: the OS will see a monitor hot-replug.
    fn set_display_mode(&mut self, mode: DisplayMode) -> Result<()>;
    /// Set brightness 0..100 if supported.
    fn set_brightness(&mut self, _percent: u8) -> Result<()> {
        Err(HalError::Unsupported("brightness"))
    }
}

/// Open the first supported real device, falling back to nothing. The daemon
/// wraps this in a hotplug poll loop (see `perch-daemon::hotplug`).
pub fn open_any() -> Result<Box<dyn Glasses>> {
    #[cfg(feature = "rokid")]
    {
        match rokid::RokidGlasses::open() {
            Ok(g) => return Ok(Box::new(g)),
            Err(HalError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    Err(HalError::NotFound)
}

#[cfg(feature = "rokid")]
pub mod rokid {
    //! Rokid Air / Max backend.
    use super::*;
    use ar_drivers::{ARGlasses, GlassesEvent};
    use nalgebra::Vector3;

    /// Rokid glasses via `ar-drivers`.
    pub struct RokidGlasses {
        inner: ar_drivers::rokid::RokidAir,
        info: DeviceInfo,
    }

    impl RokidGlasses {
        /// Open the first Rokid device on the bus.
        #[cfg(not(target_os = "android"))]
        pub fn open() -> Result<Self> {
            let inner = ar_drivers::rokid::RokidAir::new().map_err(map_err)?;
            Self::from_inner(inner)
        }

        /// Open from a file descriptor obtained via Android `UsbManager`
        /// (`UsbDeviceConnection.getFileDescriptor()`).
        #[cfg(target_os = "android")]
        pub fn open_fd(fd: isize) -> Result<Self> {
            let inner = ar_drivers::rokid::RokidAir::new(fd).map_err(map_err)?;
            Self::from_inner(inner)
        }

        fn from_inner(mut inner: ar_drivers::rokid::RokidAir) -> Result<Self> {
            let serial = inner.serial().unwrap_or_else(|_| "unknown".into());
            let model = inner.name().to_string();
            let info = DeviceInfo {
                // ar-drivers reports 23° for the Max, which is well under the
                // ~43° horizontal implied by the 50° diagonal spec. The daemon
                // overrides this from config (`display.fov_h_deg`); see
                // docs/DEVICE-ROKID-MAX.md "FOV" for how to measure yours.
                fov_h: inner.display_fov(),
                display_tilt: 0.07,
                display_delay_us: inner.display_delay(),
                has_mag: true,
                // Measured 2026-10-31 on Linux: the Max delivers 444 Hz (dt ≈ 2.25 ms),
                // not the 1 kHz quoted in ar-drivers docs. See .claude/memory/hardware-log.md.
                imu_rate_hz: if model.contains("Max") { 444 } else { 500 },
                model,
                serial,
            };
            tracing::info!(model = %info.model, serial = %info.serial, "opened Rokid glasses");
            Ok(Self { inner, info })
        }
    }

    /// ar-drivers pins nalgebra 0.32; we use 0.33. Copy the three floats.
    fn v3<T: std::ops::Index<usize, Output = f32>>(v: &T) -> Vector3<f32> {
        Vector3::new(v[0], v[1], v[2])
    }

    fn map_err(e: ar_drivers::Error) -> HalError {
        match e {
            ar_drivers::Error::NotFound => HalError::NotFound,
            ar_drivers::Error::UsbError(rusb::Error::Access) => {
                HalError::Permission("USB access denied — install udev/70-perch-rokid.rules or run with sudo".into())
            }
            ar_drivers::Error::UsbError(rusb::Error::NoDevice)
            | ar_drivers::Error::UsbError(rusb::Error::Io)
            | ar_drivers::Error::PacketTimeout => HalError::Disconnected(e.to_string()),
            other => HalError::Other(other.to_string()),
        }
    }

    fn to_mode(m: ar_drivers::DisplayMode) -> DisplayMode {
        match m {
            ar_drivers::DisplayMode::SameOnBoth | ar_drivers::DisplayMode::HalfSBS => DisplayMode::Mono,
            ar_drivers::DisplayMode::Stereo => DisplayMode::Stereo,
            ar_drivers::DisplayMode::HighRefreshRate => DisplayMode::Mono120,
            ar_drivers::DisplayMode::HighRefreshRateSBS => DisplayMode::Stereo120,
        }
    }

    fn from_mode(m: DisplayMode) -> ar_drivers::DisplayMode {
        match m {
            DisplayMode::Mono => ar_drivers::DisplayMode::SameOnBoth,
            DisplayMode::Stereo => ar_drivers::DisplayMode::Stereo,
            DisplayMode::Mono120 => ar_drivers::DisplayMode::HighRefreshRate,
            DisplayMode::Stereo120 => ar_drivers::DisplayMode::HighRefreshRateSBS,
        }
    }

    impl Glasses for RokidGlasses {
        fn info(&self) -> &DeviceInfo {
            &self.info
        }

        fn next_event(&mut self) -> Result<HalEvent> {
            // ar-drivers emits AccGyro and Magnetometer as separate events; we
            // attach the most recent mag reading to the next AccGyro sample.
            loop {
                match self.inner.read_event().map_err(map_err)? {
                    GlassesEvent::AccGyro {
                        accelerometer,
                        gyroscope,
                        timestamp,
                    } => {
                        // Measured 2026-10-31: the first ~9 packets after
                        // claiming the interface are stale (boot-clock
                        // timestamps, gyro = ±40.04 rad/s on every axis).
                        // No MEMS gyro here reads above ~35 rad/s, so drop
                        // physically impossible samples instead of integrating
                        // ~0.6 rad of garbage into the first pose.
                        let g = v3(&gyroscope);
                        if g.iter().any(|v| v.abs() > MAX_PLAUSIBLE_RATE) {
                            let n = DROPPED.with(|d| {
                                d.set(d.get() + 1);
                                d.get()
                            });
                            if n <= 20 {
                                tracing::debug!(n, ?g, "dropping implausible IMU sample");
                            }
                            continue;
                        }
                        return Ok(HalEvent::Imu(ImuSample {
                            t_us: timestamp,
                            accel: v3(&accelerometer),
                            gyro: v3(&gyroscope),
                            mag: LAST_MAG.with(|m| m.get()),
                        }));
                    }
                    GlassesEvent::Magnetometer { magnetometer, .. } => {
                        LAST_MAG.with(|m| m.set(Some(v3(&magnetometer))));
                    }
                    GlassesEvent::KeyPress(k) => return Ok(HalEvent::Button(k)),
                    GlassesEvent::ProximityNear => return Ok(HalEvent::Worn(true)),
                    GlassesEvent::ProximityFar => return Ok(HalEvent::Worn(false)),
                    GlassesEvent::AmbientLight(v) => return Ok(HalEvent::AmbientLight(v)),
                    GlassesEvent::VSync => return Ok(HalEvent::VSync),
                }
            }
        }

        fn display_mode(&mut self) -> Result<DisplayMode> {
            self.inner.get_display_mode().map(to_mode).map_err(map_err)
        }

        fn set_display_mode(&mut self, mode: DisplayMode) -> Result<()> {
            self.inner.set_display_mode(from_mode(mode)).map_err(map_err)
        }
    }

    /// Gyro readings above this (rad/s, ≈ 2000 °/s) are sensor garbage.
    const MAX_PLAUSIBLE_RATE: f32 = 35.0;

    thread_local! {
        static DROPPED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static LAST_MAG: std::cell::Cell<Option<Vector3<f32>>> = const { std::cell::Cell::new(None) };
    }
}

#[cfg(feature = "mock")]
pub mod mock {
    //! Replay backend for tests and glasses-less development.
    use super::*;
    use nalgebra::Vector3;
    use std::collections::VecDeque;

    /// One line of a recording file (JSON lines).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Record {
        /// Timestamp µs.
        pub t_us: u64,
        /// Accel m/s².
        pub accel: [f32; 3],
        /// Gyro rad/s.
        pub gyro: [f32; 3],
        /// Mag µT.
        #[serde(default)]
        pub mag: Option<[f32; 3]>,
    }

    enum Feed {
        Recorded(VecDeque<Record>),
        /// Lazily generated: `still_s` seconds still, then `turn_s` seconds at `yaw_rate`.
        Synthetic {
            still_s: f32,
            turn_s: f32,
            yaw_rate: f32,
            i: u64,
        },
    }

    /// Replays an IMU log (or a synthetic motion) in real time or as fast as possible.
    pub struct MockGlasses {
        info: DeviceInfo,
        feed: Feed,
        realtime: bool,
        started: Option<(std::time::Instant, u64)>,
        mode: DisplayMode,
    }

    fn mock_info() -> DeviceInfo {
        DeviceInfo {
            model: "Mock (Rokid Max profile)".into(),
            serial: "MOCK".into(),
            fov_h: 46f32.to_radians(),
            display_tilt: 0.07,
            display_delay_us: 13_000,
            has_mag: true,
            imu_rate_hz: 1000,
        }
    }

    impl MockGlasses {
        /// Parse a JSON-lines recording.
        pub fn from_jsonl(text: &str, realtime: bool) -> Result<Self> {
            let mut queue = VecDeque::new();
            for (i, line) in text.lines().enumerate() {
                if line.trim().is_empty() || line.starts_with('#') {
                    continue;
                }
                let r: Record = serde_json::from_str(line)
                    .map_err(|e| HalError::Other(format!("recording line {}: {e}", i + 1)))?;
                queue.push_back(r);
            }
            if queue.is_empty() {
                return Err(HalError::Other("empty recording".into()));
            }
            Ok(Self {
                info: mock_info(),
                feed: Feed::Recorded(queue),
                realtime,
                started: None,
                mode: DisplayMode::Mono120,
            })
        }

        /// Synthetic 1 kHz stream: still for `still_s`, then a constant yaw turn
        /// of `yaw_rate` rad/s for `turn_s`. Generated lazily, so an hour costs
        /// nothing. `realtime(true)` paces it at 1 kHz.
        pub fn synthetic(still_s: f32, turn_s: f32, yaw_rate: f32) -> Self {
            Self {
                info: mock_info(),
                feed: Feed::Synthetic {
                    still_s,
                    turn_s,
                    yaw_rate,
                    i: 0,
                },
                realtime: false,
                started: None,
                mode: DisplayMode::Mono120,
            }
        }

        /// Pace playback to the recording's timestamps.
        pub fn realtime(mut self, on: bool) -> Self {
            self.realtime = on;
            self
        }

        fn next_record(&mut self) -> Option<Record> {
            match &mut self.feed {
                Feed::Recorded(q) => q.pop_front(),
                Feed::Synthetic {
                    still_s,
                    turn_s,
                    yaw_rate,
                    i,
                } => {
                    let t = *i as f32 / 1000.0;
                    if t >= *still_s + *turn_s {
                        return None;
                    }
                    let g = if t < *still_s { 0.0 } else { *yaw_rate };
                    let r = Record {
                        t_us: *i * 1000,
                        accel: [0.0, G_MOCK, 0.0],
                        gyro: [0.0, g, 0.0],
                        mag: None,
                    };
                    *i += 1;
                    Some(r)
                }
            }
        }
    }

    const G_MOCK: f32 = 9.80665;

    impl Glasses for MockGlasses {
        fn info(&self) -> &DeviceInfo {
            &self.info
        }
        fn next_event(&mut self) -> Result<HalEvent> {
            let r = self
                .next_record()
                .ok_or_else(|| HalError::Disconnected("end of recording".into()))?;
            if self.realtime {
                let (t0, first_us) = *self.started.get_or_insert((std::time::Instant::now(), r.t_us));
                let target = t0 + std::time::Duration::from_micros(r.t_us.saturating_sub(first_us));
                let now = std::time::Instant::now();
                if target > now {
                    std::thread::sleep(target - now);
                }
            }
            Ok(HalEvent::Imu(ImuSample {
                t_us: r.t_us,
                accel: Vector3::from(r.accel),
                gyro: Vector3::from(r.gyro),
                mag: r.mag.map(Vector3::from),
            }))
        }
        fn display_mode(&mut self) -> Result<DisplayMode> {
            Ok(self.mode)
        }
        fn set_display_mode(&mut self, mode: DisplayMode) -> Result<()> {
            self.mode = mode;
            Ok(())
        }
        fn set_brightness(&mut self, _percent: u8) -> Result<()> {
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use perch_tracking::{Calibration, QuatExt, Tracker, TrackerConfig};

        #[test]
        fn synthetic_replay_drives_tracker() {
            let mut g = MockGlasses::synthetic(0.5, 1.0, 0.5);
            let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
            let mut last = None;
            while let Ok(ev) = g.next_event() {
                if let HalEvent::Imu(s) = ev {
                    last = Some(t.feed(s));
                }
            }
            let yaw = last.unwrap().orientation.yaw();
            assert!((yaw - 0.5).abs() < 0.02, "yaw = {yaw}");
        }

        #[test]
        fn jsonl_parse_errors_are_reported() {
            assert!(MockGlasses::from_jsonl("not json", false).is_err());
            assert!(MockGlasses::from_jsonl("# comment only\n", false).is_err());
        }
    }
}
