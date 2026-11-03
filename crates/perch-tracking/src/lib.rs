//! `perch-tracking` — turns raw IMU samples from the glasses into a stable,
//! drift-corrected 3DOF head pose.
//!
//! Pipeline:
//!
//! ```text
//!  accel/gyro/(mag) ─► bias subtract ─► Mahony AHRS ─► yaw offset (re-centre)
//!                          ▲               │
//!                   stationary gyro        └─► prediction (display latency)
//!                   calibration
//! ```
//!
//! Why Mahony rather than Madgwick/EKF: it is tiny, deterministic, trivially
//! tunable (`kp`, `ki`), and its integral term *is* an online gyro-bias
//! estimator, which is exactly the thing that makes or breaks a 3DOF pinned
//! display. Yaw is unobservable from accelerometer alone, so yaw drift is
//! handled by (1) a one-shot stationary bias calibration (same idea as the
//! 60-second `--cal` in spidgets-3dof), (2) the slow integral term, and (3)
//! optionally the magnetometer, which on Rokid Max is noisy and **off by
//! default**.
//!
//! All frames are RUB (X right, Y up, Z back) as delivered by `ar-drivers`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use nalgebra::{UnitQuaternion, Vector3};
use perch_proto::{CalibrationState, Pose};
use serde::{Deserialize, Serialize};

/// Standard gravity, m/s².
pub const G: f32 = 9.80665;

/// One synchronised IMU sample from the glasses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImuSample {
    /// Device timestamp, microseconds. Must be monotonic within a session.
    pub t_us: u64,
    /// Specific force in head frame, m/s². Upright & still → (0, +9.81, 0).
    pub accel: Vector3<f32>,
    /// Angular rate in head frame, rad/s, right-handed.
    pub gyro: Vector3<f32>,
    /// Magnetic field, µT, if the device reports it.
    pub mag: Option<Vector3<f32>>,
}

/// Persisted per-device calibration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Calibration {
    /// Gyro zero-rate offset, rad/s, head frame.
    pub gyro_bias: Vector3<f32>,
    /// Accelerometer scale correction (1.0 = none).
    pub accel_scale: f32,
    /// Number of samples the bias was averaged over.
    pub samples: u32,
}

impl Calibration {
    /// A calibration with no corrections.
    pub fn none() -> Self {
        Self {
            gyro_bias: Vector3::zeros(),
            accel_scale: 1.0,
            samples: 0,
        }
    }
}

/// Tuning knobs. Defaults are tuned for Rokid Max at ~1 kHz IMU rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackerConfig {
    /// Proportional gain of the gravity correction. Higher = snappier tilt
    /// correction, more sensitive to linear acceleration (walking, nodding).
    pub kp: f32,
    /// Integral gain = online gyro-bias learning rate. Keep small.
    pub ki: f32,
    /// Accel samples whose magnitude differs from g by more than this
    /// fraction are ignored for correction (they contain motion, not gravity).
    pub accel_gate: f32,
    /// Use magnetometer for yaw correction.
    pub use_mag: bool,
    /// Magnetometer correction gain (only if `use_mag`).
    pub kp_mag: f32,
    /// If a gap between samples exceeds this (seconds), skip integration
    /// (USB stall, suspend) instead of applying a huge rotation.
    pub max_dt: f32,
    /// Default prediction horizon in seconds, added on top of what the caller
    /// passes to [`Tracker::pose_predicted`]. Rokid Max display delay is
    /// ~13 ms per `ar-drivers`.
    pub base_latency_s: f32,
    /// Stationary calibration is rejected if the peak angular rate during the
    /// run exceeds this (deg/s) above the mean. Measured on a real desk the
    /// Max sees 2–4 °/s vibration peaks at 444 Hz while visibly still, so the
    /// default is deliberately loose; real head motion is tens of °/s.
    #[serde(default = "default_calib_max_rate_deg_s")]
    pub calib_max_rate_deg_s: f32,
    /// Keep learning the gyro bias whenever the glasses are still (gravity
    /// cannot observe yaw bias, and the bias wanders with temperature after a
    /// one-off calibration). Off = only the explicit calibration applies.
    #[serde(default = "default_true")]
    pub auto_bias: bool,
    /// A sample counts as "still" if |gyro − bias| is below this (deg/s) and
    /// the accelerometer reads ≈ g.
    #[serde(default = "default_auto_bias_max_rate_deg_s")]
    pub auto_bias_max_rate_deg_s: f32,
    /// Seconds of uninterrupted stillness before one bias update.
    #[serde(default = "default_auto_bias_window_s")]
    pub auto_bias_window_s: f32,
    /// Time constant (seconds) of the bias update; larger = slower, safer.
    #[serde(default = "default_auto_bias_tau_s")]
    pub auto_bias_tau_s: f32,
}

fn default_calib_max_rate_deg_s() -> f32 {
    5.0
}
fn default_true() -> bool {
    true
}
fn default_auto_bias_max_rate_deg_s() -> f32 {
    1.5
}
fn default_auto_bias_window_s() -> f32 {
    2.0
}
fn default_auto_bias_tau_s() -> f32 {
    15.0
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            kp: 1.0,
            ki: 0.02,
            accel_gate: 0.15,
            use_mag: false,
            kp_mag: 0.05,
            max_dt: 0.1,
            base_latency_s: 0.013,
            calib_max_rate_deg_s: default_calib_max_rate_deg_s(),
            auto_bias: default_true(),
            auto_bias_max_rate_deg_s: default_auto_bias_max_rate_deg_s(),
            auto_bias_window_s: default_auto_bias_window_s(),
            auto_bias_tau_s: default_auto_bias_tau_s(),
        }
    }
}

/// Stationary gyro-bias calibration in progress.
#[derive(Debug, Clone)]
struct CalibRun {
    start_us: u64,
    duration_us: u64,
    sum_gyro: Vector3<f64>,
    sum_accel_mag: f64,
    n: u32,
    /// Max |gyro| seen; if the user moved, we reject the run.
    max_rate: f32,
}

/// Running window of raw gyro samples while the glasses look still.
#[derive(Debug, Clone, Default)]
struct StillWindow {
    start_us: Option<u64>,
    sum: Vector3<f64>,
    n: u32,
}

/// The tracker. One per connected glasses.
#[derive(Debug, Clone)]
pub struct Tracker {
    cfg: TrackerConfig,
    cal: Calibration,
    still: StillWindow,
    /// Head → world (before re-centre offset).
    q: UnitQuaternion<f32>,
    /// Mahony integral error term (acts as bias estimate delta).
    integral: Vector3<f32>,
    /// Re-centre: rotation about world Y applied to the output.
    yaw_offset: UnitQuaternion<f32>,
    last_t_us: Option<u64>,
    last_gyro: Vector3<f32>,
    initialised: bool,
    calib: Option<CalibRun>,
    state: CalibrationState,
    samples_seen: u64,
    /// Peak |gyro| of the most recently finished calibration run (rad/s).
    last_calib_peak_rate: f32,
}

impl Tracker {
    /// Create a tracker with the given config and (possibly empty) calibration.
    pub fn new(cfg: TrackerConfig, cal: Calibration) -> Self {
        let state = if cal.samples > 0 {
            CalibrationState::Ready
        } else {
            CalibrationState::Uncalibrated
        };
        Self {
            cfg,
            cal,
            still: StillWindow::default(),
            q: UnitQuaternion::identity(),
            integral: Vector3::zeros(),
            yaw_offset: UnitQuaternion::identity(),
            last_t_us: None,
            last_gyro: Vector3::zeros(),
            initialised: false,
            calib: None,
            state,
            samples_seen: 0,
            last_calib_peak_rate: 0.0,
        }
    }

    /// Current calibration (gyro bias etc.), suitable for persisting.
    pub fn calibration(&self) -> Calibration {
        self.cal
    }

    /// Peak angular rate (rad/s) seen during the last finished calibration
    /// run, accepted or rejected. Useful for telling the user *why* a run was
    /// rejected.
    pub fn last_calibration_peak_rate(&self) -> f32 {
        self.last_calib_peak_rate
    }

    /// Current calibration state for the UI.
    pub fn calibration_state(&self) -> CalibrationState {
        self.state
    }

    /// Config accessor.
    pub fn config(&self) -> &TrackerConfig {
        &self.cfg
    }

    /// Replace config live.
    pub fn set_config(&mut self, cfg: TrackerConfig) {
        self.cfg = cfg;
    }

    /// Begin a stationary calibration lasting `seconds`. The glasses must not
    /// move; the run is rejected (state goes back to the previous value) if the
    /// peak angular rate exceeds [`TrackerConfig::calib_max_rate_deg_s`].
    pub fn start_calibration(&mut self, seconds: f32) {
        self.calib = Some(CalibRun {
            start_us: self.last_t_us.unwrap_or(0),
            duration_us: (seconds.max(1.0) * 1e6) as u64,
            sum_gyro: Vector3::zeros(),
            sum_accel_mag: 0.0,
            n: 0,
            max_rate: 0.0,
        });
        self.state = CalibrationState::Calibrating { percent: 0 };
    }

    /// Make the current gaze direction yaw = 0. Pitch/roll stay gravity-aligned.
    pub fn recenter(&mut self) {
        let yaw = world_yaw(&self.q);
        self.yaw_offset = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -yaw);
    }

    /// Feed one IMU sample. Returns the new pose (always, even during
    /// calibration, with reduced confidence).
    pub fn feed(&mut self, s: ImuSample) -> Pose {
        self.samples_seen += 1;

        if let Some(run) = self.calib.as_mut() {
            if run.n == 0 {
                run.start_us = s.t_us;
            }
            run.sum_gyro += s.gyro.cast::<f64>();
            run.sum_accel_mag += s.accel.norm() as f64;
            run.n += 1;
            run.max_rate = run.max_rate.max(s.gyro.norm());
            let elapsed = s.t_us.saturating_sub(run.start_us);
            let pct = ((elapsed * 100) / run.duration_us.max(1)).min(100) as u8;
            self.state = CalibrationState::Calibrating { percent: pct };
            if elapsed >= run.duration_us {
                let run = self.calib.take().unwrap();
                self.last_calib_peak_rate = run.max_rate;
                let moved = run.max_rate
                    > self.cfg.calib_max_rate_deg_s.to_radians() + run.sum_gyro.norm() as f32 / run.n as f32;
                if moved || run.n < 50 {
                    // Rejected: keep the old calibration.
                    self.state = if self.cal.samples > 0 {
                        CalibrationState::Ready
                    } else {
                        CalibrationState::Uncalibrated
                    };
                } else {
                    let n = run.n as f64;
                    self.cal = Calibration {
                        gyro_bias: (run.sum_gyro / n).cast::<f32>(),
                        accel_scale: (G as f64 / (run.sum_accel_mag / n)) as f32,
                        samples: run.n,
                    };
                    self.integral = Vector3::zeros();
                    self.state = CalibrationState::Ready;
                }
            }
        }

        if self.cfg.auto_bias && self.calib.is_none() {
            self.learn_bias_when_still(&s);
        }

        let gyro = s.gyro - self.cal.gyro_bias;
        let accel = s.accel * self.cal.accel_scale;

        if !self.initialised {
            // Align initial orientation to gravity so the first frame isn't tilted.
            if accel.norm() > 0.5 * G {
                self.q = align_to_gravity(&accel);
                self.initialised = true;
            }
            self.last_t_us = Some(s.t_us);
            self.last_gyro = gyro;
            return self.output(s.t_us, gyro);
        }

        let dt = match self.last_t_us {
            Some(t0) if s.t_us > t0 => (s.t_us - t0) as f32 * 1e-6,
            _ => 0.0,
        };
        self.last_t_us = Some(s.t_us);
        self.last_gyro = gyro;

        if dt <= 0.0 || dt > self.cfg.max_dt {
            return self.output(s.t_us, gyro);
        }

        // --- Mahony correction from gravity -------------------------------
        let mut omega = gyro;
        let a_norm = accel.norm();
        if a_norm > 0.0 && ((a_norm - G) / G).abs() < self.cfg.accel_gate {
            let a = accel / a_norm;
            // Gravity (world +Y) expressed in head frame per current estimate.
            let v = self.q.inverse() * Vector3::y();
            // Error is the rotation that would move estimate onto measurement.
            let e = a.cross(&v);
            self.integral += e * self.cfg.ki * dt;
            omega += e * self.cfg.kp + self.integral;
        }

        // --- Optional magnetometer yaw correction ----------------------
        if self.cfg.use_mag {
            if let Some(m) = s.mag {
                if m.norm() > 1.0 {
                    // Project measured field onto the world horizontal plane and
                    // compare with +Z (we define "north" at init as whatever we saw).
                    let m_world = self.q * m.normalize();
                    let m_h = Vector3::new(m_world.x, 0.0, m_world.z);
                    if m_h.norm() > 0.2 {
                        let m_h = m_h.normalize();
                        let ref_h = Vector3::z();
                        let e_world = m_h.cross(&ref_h);
                        let e_head = self.q.inverse() * e_world;
                        omega += e_head * self.cfg.kp_mag;
                    }
                }
            }
        }

        // --- Integrate ------------------------------------------------
        self.q = integrate(&self.q, &omega, dt);
        self.output(s.t_us, gyro)
    }

    /// Online bias refinement: while every sample in a window of
    /// `auto_bias_window_s` is below the stillness threshold (with gravity
    /// alone on the accelerometer), move the bias toward the window's mean
    /// raw gyro with time constant `auto_bias_tau_s`. Motion resets the window.
    fn learn_bias_when_still(&mut self, s: &ImuSample) {
        let thresh = self.cfg.auto_bias_max_rate_deg_s.to_radians();
        let still = (s.gyro - self.cal.gyro_bias).norm() < thresh
            && ((s.accel.norm() * self.cal.accel_scale - G) / G).abs() < self.cfg.accel_gate;
        if !still {
            self.still = StillWindow::default();
            return;
        }
        let w = &mut self.still;
        let start = *w.start_us.get_or_insert(s.t_us);
        w.sum += s.gyro.cast::<f64>();
        w.n += 1;
        let elapsed_s = s.t_us.saturating_sub(start) as f32 * 1e-6;
        if elapsed_s >= self.cfg.auto_bias_window_s && w.n >= 50 {
            let mean = (w.sum / w.n as f64).cast::<f32>();
            let alpha = (elapsed_s / self.cfg.auto_bias_tau_s.max(elapsed_s)).min(1.0);
            self.cal.gyro_bias += (mean - self.cal.gyro_bias) * alpha;
            if self.cal.samples == 0 {
                self.cal.samples = w.n;
            }
            self.still = StillWindow::default();
        }
    }

    /// Pose predicted `extra_s` seconds ahead (plus `base_latency_s`) by
    /// extrapolating the last angular velocity. Call this right before
    /// rendering a frame.
    pub fn pose_predicted(&self, extra_s: f32) -> Pose {
        let horizon = self.cfg.base_latency_s + extra_s;
        let q = integrate(&self.q, &self.last_gyro, horizon);
        let mut p = self.make_pose(self.last_t_us.unwrap_or(0), q, self.last_gyro);
        p.t_us += (horizon * 1e6) as u64;
        p
    }

    fn output(&self, t_us: u64, gyro: Vector3<f32>) -> Pose {
        self.make_pose(t_us, self.q, gyro)
    }

    fn make_pose(&self, t_us: u64, q: UnitQuaternion<f32>, gyro: Vector3<f32>) -> Pose {
        let confidence = match self.state {
            CalibrationState::Ready => 1.0,
            CalibrationState::Calibrating { .. } => 0.5,
            CalibrationState::Uncalibrated => 0.7,
        };
        Pose {
            t_us,
            orientation: self.yaw_offset * q,
            angular_velocity: gyro,
            confidence: if self.initialised { confidence } else { 0.0 },
        }
    }
}

/// Integrate body-frame angular velocity into a head→world quaternion.
pub fn integrate(q: &UnitQuaternion<f32>, omega: &Vector3<f32>, dt: f32) -> UnitQuaternion<f32> {
    let angle = omega.norm() * dt;
    if angle < 1e-9 {
        return *q;
    }
    let axis = nalgebra::Unit::new_normalize(*omega);
    let dq = UnitQuaternion::from_axis_angle(&axis, angle);
    // Body-frame rate: post-multiply.
    (q * dq).renormalize_fast_owned()
}

/// Orientation whose inverse maps the measured accel direction to world +Y,
/// with zero yaw.
pub fn align_to_gravity(accel: &Vector3<f32>) -> UnitQuaternion<f32> {
    let a = accel.normalize();
    // We want q such that q * a == +Y.
    UnitQuaternion::rotation_between(&a, &Vector3::y()).unwrap_or_else(UnitQuaternion::identity)
}

/// Yaw (rotation about world +Y) of a head→world quaternion, radians,
/// positive = turning left. Independent of pitch/roll decomposition issues:
/// we project the head's forward vector (-Z) onto the horizontal plane.
pub fn world_yaw(q: &UnitQuaternion<f32>) -> f32 {
    let fwd = q * -Vector3::z();
    // atan2(x, -z): forward = (0,0,-1) → 0; turning left → fwd.x < 0 in RUB?
    // Turning left (positive rotation about +Y) maps -Z to -X, so yaw(+) ⇒ fwd.x < 0.
    (-fwd.x).atan2(-fwd.z)
}

/// Pitch of the head forward vector above the horizon, radians, + = up.
pub fn world_pitch(q: &UnitQuaternion<f32>) -> f32 {
    let fwd = q * -Vector3::z();
    fwd.y.clamp(-1.0, 1.0).asin()
}

/// Convenience trait so helper code can ask any quaternion for yaw/pitch.
pub trait QuatExt {
    /// See [`world_yaw`].
    fn yaw(&self) -> f32;
    /// See [`world_pitch`].
    fn pitch(&self) -> f32;
}
impl QuatExt for UnitQuaternion<f32> {
    fn yaw(&self) -> f32 {
        world_yaw(self)
    }
    fn pitch(&self) -> f32 {
        world_pitch(self)
    }
}

trait RenormExt {
    fn renormalize_fast_owned(self) -> Self;
}
impl RenormExt for UnitQuaternion<f32> {
    fn renormalize_fast_owned(mut self) -> Self {
        self.renormalize_fast();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still_sample(t_us: u64, bias: Vector3<f32>) -> ImuSample {
        ImuSample {
            t_us,
            accel: Vector3::new(0.0, G, 0.0),
            gyro: bias,
            mag: None,
        }
    }

    #[test]
    fn still_glasses_stay_still() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        let mut p = Pose::identity();
        for i in 0..2000 {
            p = t.feed(still_sample(i * 1000, Vector3::zeros()));
        }
        assert!(world_yaw(&p.orientation).abs() < 1e-4);
        assert!(world_pitch(&p.orientation).abs() < 1e-4);
    }

    #[test]
    fn pure_yaw_rotation_integrates() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        // 1 rad/s about +Y for 0.5 s at 1 kHz
        let mut p = Pose::identity();
        for i in 0..=500 {
            p = t.feed(ImuSample {
                t_us: i * 1000,
                accel: Vector3::new(0.0, G, 0.0),
                gyro: Vector3::new(0.0, 1.0, 0.0),
                mag: None,
            });
        }
        let yaw = world_yaw(&p.orientation);
        assert!((yaw - 0.5).abs() < 0.01, "yaw = {yaw}");
    }

    #[test]
    fn calibration_learns_bias_and_removes_drift() {
        let bias = Vector3::new(0.002, 0.01, -0.001); // ~0.57°/s yaw drift
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        t.start_calibration(2.0);
        let mut i = 0u64;
        while !matches!(t.calibration_state(), CalibrationState::Ready) {
            t.feed(still_sample(i * 1000, bias));
            i += 1;
            assert!(i < 5000, "calibration never finished");
        }
        let cal = t.calibration();
        assert!((cal.gyro_bias - bias).norm() < 1e-5);
        // After calibration, 10 s of "still but biased" input must not drift.
        t.recenter();
        let mut p = Pose::identity();
        for j in 0..10_000 {
            p = t.feed(still_sample((i + j) * 1000, bias));
        }
        assert!(
            world_yaw(&p.orientation).abs() < 0.002,
            "yaw drift {}",
            world_yaw(&p.orientation)
        );
    }

    #[test]
    fn calibration_rejected_if_moved() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        t.start_calibration(1.0);
        for i in 0..1200 {
            let g = if i == 600 {
                Vector3::new(0.0, 0.5, 0.0)
            } else {
                Vector3::zeros()
            };
            t.feed(still_sample(i * 1000, g));
        }
        assert_eq!(t.calibration_state(), CalibrationState::Uncalibrated);
        assert_eq!(t.calibration().samples, 0);
    }

    /// Without any explicit calibration, a biased-but-still IMU must stop
    /// drifting once the online bias learner has had a minute of stillness.
    #[test]
    fn auto_bias_removes_yaw_drift_while_still() {
        let bias = Vector3::new(-0.0009, 0.0035, -0.0001); // the S24 run's real bias
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        // Uncorrected drift rate over the first 10 s.
        let mut p = t.feed(still_sample(0, bias));
        let y0 = world_yaw(&p.orientation);
        for i in 1..(10 * 444) {
            p = t.feed(still_sample(i as u64 * 2250, bias));
        }
        let early_rate = (world_yaw(&p.orientation) - y0).abs() / 10.0;
        // Let it learn for 60 s.
        for i in (10 * 444)..(70 * 444) {
            p = t.feed(still_sample(i as u64 * 2250, bias));
        }
        let y1 = world_yaw(&p.orientation);
        for i in (70 * 444)..(80 * 444) {
            p = t.feed(still_sample(i as u64 * 2250, bias));
        }
        let late_rate = (world_yaw(&p.orientation) - y1).abs() / 10.0;
        assert!(
            early_rate > 1e-3,
            "test needs a visible uncorrected drift, got {early_rate}"
        );
        assert!(
            late_rate < early_rate * 0.05,
            "drift {late_rate} rad/s should be < 5% of {early_rate}"
        );
        assert!((t.calibration().gyro_bias - bias).norm() < 2e-4);
    }

    /// Measured on a Rokid Max resting on a desk (2026-10-31): visibly still,
    /// yet raw gyro peaks of 2–4 °/s from desk/cable vibration. That must not
    /// reject a run; a real nudge (≫ threshold) must.
    #[test]
    fn calibration_tolerates_desk_vibration_but_not_a_nudge() {
        let bias = Vector3::new(0.0, 0.0044, -0.0003);
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        t.start_calibration(1.0);
        for i in 0..1200u64 {
            // 3.5 °/s peak-to-zero wobble on z, like the desk measurement.
            let wobble = 3.5f32.to_radians() * ((i as f32) * 0.3).sin();
            t.feed(still_sample(i * 2250, bias + Vector3::new(0.0, 0.0, wobble)));
        }
        assert_eq!(t.calibration_state(), CalibrationState::Ready);
        assert!((t.calibration().gyro_bias.y - bias.y).abs() < 1e-3);

        let strict = TrackerConfig {
            calib_max_rate_deg_s: 2.0,
            ..Default::default()
        };
        let mut t = Tracker::new(strict, Calibration::none());
        t.start_calibration(1.0);
        for i in 0..1200u64 {
            let wobble = 3.5f32.to_radians() * ((i as f32) * 0.3).sin();
            t.feed(still_sample(i * 2250, bias + Vector3::new(0.0, 0.0, wobble)));
        }
        assert_eq!(t.calibration_state(), CalibrationState::Uncalibrated);
        assert!(t.last_calibration_peak_rate() > 3.0f32.to_radians());
    }

    #[test]
    fn recenter_zeroes_yaw_keeps_pitch() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        for i in 0..=1000 {
            t.feed(ImuSample {
                t_us: i * 1000,
                accel: Vector3::new(0.0, G, 0.0),
                gyro: Vector3::new(0.0, 0.8, 0.0),
                mag: None,
            });
        }
        assert!(world_yaw(&t.feed(still_sample(1_001_000, Vector3::zeros())).orientation) > 0.7);
        t.recenter();
        let p = t.feed(still_sample(1_002_000, Vector3::zeros()));
        assert!(world_yaw(&p.orientation).abs() < 1e-3);
    }

    #[test]
    fn gravity_correction_fixes_initial_tilt() {
        // Start with glasses pitched 20° up: accel = rotate (0,g,0).
        let tilt = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 20f32.to_radians());
        let accel_head = tilt.inverse() * Vector3::new(0.0, G, 0.0);
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        let mut p = Pose::identity();
        for i in 0..3000 {
            p = t.feed(ImuSample {
                t_us: i * 1000,
                accel: accel_head,
                gyro: Vector3::zeros(),
                mag: None,
            });
        }
        let pitch = world_pitch(&p.orientation).to_degrees();
        assert!((pitch - 20.0).abs() < 0.5, "pitch = {pitch}");
    }

    #[test]
    fn prediction_moves_ahead() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        for i in 0..=100 {
            t.feed(ImuSample {
                t_us: i * 1000,
                accel: Vector3::new(0.0, G, 0.0),
                gyro: Vector3::new(0.0, 2.0, 0.0),
                mag: None,
            });
        }
        let now = t.feed(still_sample(101_000, Vector3::new(0.0, 2.0, 0.0)));
        let ahead = t.pose_predicted(0.010);
        // 23 ms horizon at 2 rad/s ≈ 0.046 rad further.
        let d = world_yaw(&ahead.orientation) - world_yaw(&now.orientation);
        assert!((d - 0.046).abs() < 0.005, "d = {d}");
    }

    #[test]
    fn large_gap_is_skipped() {
        let mut t = Tracker::new(TrackerConfig::default(), Calibration::none());
        t.feed(still_sample(0, Vector3::zeros()));
        t.feed(still_sample(1000, Vector3::zeros()));
        // 5 s gap with a huge rate must not spin the world.
        let p = t.feed(ImuSample {
            t_us: 5_001_000,
            accel: Vector3::new(0.0, G, 0.0),
            gyro: Vector3::new(0.0, 10.0, 0.0),
            mag: None,
        });
        assert!(world_yaw(&p.orientation).abs() < 1e-3);
    }

    #[test]
    fn calibration_serialises() {
        let c = Calibration {
            gyro_bias: Vector3::new(1e-3, 2e-3, 3e-3),
            accel_scale: 1.01,
            samples: 1000,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Calibration = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
