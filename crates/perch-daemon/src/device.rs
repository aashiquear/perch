//! Glasses device thread: hotplug loop → HAL → tracker → pose publishing.
//!
//! Runs on a plain OS thread (the HAL blocks on USB reads at up to 1 kHz).
//! Poses are published through a `tokio::sync::watch` channel so any number
//! of consumers (compositor, control server, stream socket) read the latest
//! without queueing.

use crate::config::{Config, Paths};
use perch_hal::{Glasses, HalError, HalEvent};
use perch_proto::{CalibrationState, DisplayMode, Pose};
use perch_tracking::{Calibration, Tracker};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// What the device thread reports upward.
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// Connected.
    Connected {
        /// Model.
        model: String,
        /// Serial.
        serial: String,
        /// Mode after applying config.
        mode: Option<DisplayMode>,
    },
    /// Disconnected.
    Disconnected,
    /// Calibration state changed.
    Calibration(CalibrationState),
    /// A stationary calibration run was rejected (glasses moved).
    CalibrationRejected {
        /// Peak angular rate seen during the run, deg/s.
        peak_deg_s: f32,
    },
    /// Temple button.
    Button(u8),
    /// Worn / taken off (reserved: auto-dim / pause when taken off).
    #[allow(dead_code)]
    Worn(bool),
}

/// Commands to the device thread.
#[derive(Debug, Clone)]
pub enum DeviceCommand {
    /// Recenter.
    Recenter,
    /// Calibrate.
    Calibrate(u8),
    /// Display mode.
    SetDisplayMode(DisplayMode),
    /// Brightness.
    SetBrightness(u8),
}

/// How to obtain a device.
#[allow(dead_code)]
pub enum DeviceSource {
    /// Real hardware via `perch_hal::open_any`.
    Hardware,
    /// A mock built by this closure (recording or synthetic).
    Mock(Box<dyn Fn() -> Box<dyn Glasses> + Send>),
}

/// Handle to the device thread.
#[allow(dead_code)]
pub struct DeviceHandle {
    /// Latest pose.
    pub pose_rx: watch::Receiver<Pose>,
    /// Device events.
    pub events_rx: mpsc::UnboundedReceiver<DeviceEvent>,
    /// Command sender.
    pub cmd_tx: std::sync::mpsc::Sender<DeviceCommand>,
    /// Shared tracker so the render loop can ask for predicted poses.
    pub tracker: Arc<Mutex<Tracker>>,
}

/// Spawn the device thread.
pub fn spawn(cfg: Config, paths: Option<Paths>, source: DeviceSource) -> DeviceHandle {
    let (pose_tx, pose_rx) = watch::channel(Pose::identity());
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<DeviceCommand>();
    let tracker = Arc::new(Mutex::new(Tracker::new(cfg.tracker, Calibration::none())));
    let t2 = Arc::clone(&tracker);
    std::thread::Builder::new()
        .name("perch-device".into())
        .spawn(move || device_loop(cfg, paths, source, pose_tx, events_tx, cmd_rx, t2))
        .expect("spawn device thread");
    DeviceHandle {
        pose_rx,
        events_rx,
        cmd_tx,
        tracker,
    }
}

fn parse_mode(s: &str) -> Option<DisplayMode> {
    Some(match s.to_ascii_lowercase().as_str() {
        "mono" => DisplayMode::Mono,
        "stereo" => DisplayMode::Stereo,
        "mono120" => DisplayMode::Mono120,
        "stereo120" => DisplayMode::Stereo120,
        _ => return None,
    })
}

fn device_loop(
    cfg: Config,
    paths: Option<Paths>,
    source: DeviceSource,
    pose_tx: watch::Sender<Pose>,
    events_tx: mpsc::UnboundedSender<DeviceEvent>,
    cmd_rx: std::sync::mpsc::Receiver<DeviceCommand>,
    tracker: Arc<Mutex<Tracker>>,
) {
    loop {
        // --- acquire ---------------------------------------------------
        let mut dev: Box<dyn Glasses> = match &source {
            DeviceSource::Hardware => match perch_hal::open_any() {
                Ok(d) => d,
                Err(HalError::NotFound) => {
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }
                Err(e) => {
                    tracing::error!("open glasses: {e}");
                    std::thread::sleep(Duration::from_millis(2000));
                    continue;
                }
            },
            DeviceSource::Mock(make) => make(),
        };
        let info = dev.info().clone();

        // Load per-serial calibration.
        let cal = paths
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p.calibration_file(&info.serial)).ok())
            .and_then(|s| serde_json::from_str::<Calibration>(&s).ok())
            .unwrap_or_else(Calibration::none);
        {
            let mut t = tracker.lock().unwrap();
            let mut tc = cfg.tracker;
            tc.base_latency_s = info.display_delay_us as f32 * 1e-6 + cfg.display.extra_prediction_ms * 1e-3;
            *t = Tracker::new(tc, cal);
        }

        let mode = parse_mode(&cfg.display.mode).and_then(|m| {
            if let Err(e) = dev.set_display_mode(m) {
                tracing::warn!("set display mode: {e}");
            }
            dev.display_mode().ok()
        });
        let _ = events_tx.send(DeviceEvent::Connected {
            model: info.model.clone(),
            serial: info.serial.clone(),
            mode,
        });
        let mut last_state = tracker.lock().unwrap().calibration_state();
        let _ = events_tx.send(DeviceEvent::Calibration(last_state));
        if last_state == CalibrationState::Uncalibrated {
            tracing::info!(
                "no calibration for {} — run `perch calibrate` with the glasses on a table",
                info.serial
            );
        }

        // --- stream ----------------------------------------------------
        let mut cal_samples_before = 0u32;
        loop {
            while let Ok(c) = cmd_rx.try_recv() {
                match c {
                    DeviceCommand::Recenter => tracker.lock().unwrap().recenter(),
                    DeviceCommand::Calibrate(s) => {
                        let mut t = tracker.lock().unwrap();
                        cal_samples_before = t.calibration().samples;
                        t.start_calibration(s as f32);
                    }
                    DeviceCommand::SetDisplayMode(m) => {
                        if let Err(e) = dev.set_display_mode(m) {
                            tracing::warn!("display mode: {e}");
                        }
                    }
                    DeviceCommand::SetBrightness(b) => {
                        if let Err(e) = dev.set_brightness(b) {
                            tracing::debug!("brightness: {e}");
                        }
                    }
                }
            }
            match dev.next_event() {
                Ok(HalEvent::Imu(s)) => {
                    let (pose, state) = {
                        let mut t = tracker.lock().unwrap();
                        let p = t.feed(s);
                        (p, t.calibration_state())
                    };
                    let _ = pose_tx.send(pose);
                    if state != last_state {
                        if matches!(last_state, CalibrationState::Calibrating { .. })
                            && !matches!(state, CalibrationState::Calibrating { .. })
                        {
                            let t = tracker.lock().unwrap();
                            if t.calibration().samples == cal_samples_before {
                                let peak_deg_s = t.last_calibration_peak_rate().to_degrees();
                                tracing::warn!(
                                    peak_deg_s,
                                    limit_deg_s = cfg.tracker.calib_max_rate_deg_s,
                                    "calibration rejected: glasses moved"
                                );
                                let _ = events_tx.send(DeviceEvent::CalibrationRejected { peak_deg_s });
                            }
                        }
                        if state == CalibrationState::Ready && !matches!(last_state, CalibrationState::Uncalibrated) {
                            // Calibration just finished: persist.
                            if let Some(p) = &paths {
                                let cal = tracker.lock().unwrap().calibration();
                                let _ = std::fs::create_dir_all(&p.dir);
                                if let Ok(j) = serde_json::to_string_pretty(&cal) {
                                    let _ = std::fs::write(p.calibration_file(&info.serial), j);
                                }
                            }
                        }
                        last_state = state;
                        let _ = events_tx.send(DeviceEvent::Calibration(state));
                    }
                }
                Ok(HalEvent::Button(b)) => {
                    let _ = events_tx.send(DeviceEvent::Button(b));
                }
                Ok(HalEvent::Worn(w)) => {
                    let _ = events_tx.send(DeviceEvent::Worn(w));
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("glasses stream ended: {e}");
                    let _ = events_tx.send(DeviceEvent::Disconnected);
                    break;
                }
            }
        }
        if matches!(source, DeviceSource::Mock(_)) {
            // A finished recording means we're done; don't loop forever.
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
