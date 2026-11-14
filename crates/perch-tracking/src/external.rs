//! Head tracking from a sensor that already reports an orientation — a
//! headset with its own fusion (Bose AR game-rotation vector, AirPods
//! attitude) — instead of the raw IMU the [`crate::Tracker`] fuses.
//!
//! Such a source differs from the glasses' IMU in three ways this module
//! deals with: its axes are its own ([`SensorFrame`]), it sits on the head at
//! an unknown tilt (removed at [`ExternalTracker::recenter`]), and it is slow
//! and bursty — tens of samples a second, several arriving in one radio
//! packet — so the pose is extrapolated from the sample's own clock.

use crate::{integrate, world_yaw};
use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use perch_proto::Pose;

/// How a sensor's axes map onto Perch's RUB frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorFrame {
    /// Rotation taking sensor-body coordinates to head RUB coordinates.
    pub body_to_rub: UnitQuaternion<f32>,
    /// Rotation taking sensor-world coordinates to world RUB (Y up).
    pub world_to_rub: UnitQuaternion<f32>,
}

impl SensorFrame {
    /// Bose AR headsets: body X right, Y backward, Z down; world Z up.
    /// *Measured* on a Noise Cancelling Headphones 700 on 2026-11-14 (three
    /// marked poses, `.claude/memory/hardware-log.md`); it also matches the
    /// convention Bose documented for the platform.
    pub fn bose_ar() -> Self {
        Self {
            // (x, y, z) → (x, −z, y)
            body_to_rub: UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f32::consts::FRAC_PI_2),
            // (x, y, z) → (x, z, −y)
            world_to_rub: UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -std::f32::consts::FRAC_PI_2),
        }
    }

    /// A source that already speaks RUB.
    pub fn rub() -> Self {
        Self {
            body_to_rub: UnitQuaternion::identity(),
            world_to_rub: UnitQuaternion::identity(),
        }
    }

    /// Sensor quaternion (body → sensor world) as head → world in RUB,
    /// still carrying the sensor's mounting tilt and arbitrary yaw.
    pub fn to_rub(&self, q: &UnitQuaternion<f32>) -> UnitQuaternion<f32> {
        self.world_to_rub * q * self.body_to_rub.inverse()
    }
}

/// Tuning for [`ExternalTracker`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExternalConfig {
    /// Display pipeline delay added to every prediction, seconds.
    pub base_latency_s: f32,
    /// Longest extrapolation; beyond it the pose holds (a stalled radio link
    /// must not swing the picture away).
    pub max_prediction_s: f32,
    /// Low-pass on the angular-velocity estimate, 0..1 per sample (1 = none).
    pub rate_smoothing: f32,
    /// Rates below this are treated as a still head (rad/s): quantisation
    /// noise must not make a pinned window shimmer.
    pub rate_deadband: f32,
}

impl Default for ExternalConfig {
    fn default() -> Self {
        Self {
            base_latency_s: 0.02,
            max_prediction_s: 0.12,
            rate_smoothing: 0.5,
            rate_deadband: 0.02,
        }
    }
}

/// Turns timestamped orientations into head poses in Perch's convention.
#[derive(Debug, Clone)]
pub struct ExternalTracker {
    frame: SensorFrame,
    cfg: ExternalConfig,
    /// Sensor mounting tilt, removed on the body side.
    mount: UnitQuaternion<f32>,
    /// World yaw that the recentred gaze defines as zero.
    yaw_zero: UnitQuaternion<f32>,
    /// Last sample in RUB, before mount/yaw correction.
    raw: Option<UnitQuaternion<f32>>,
    /// Head-frame angular velocity estimate, rad/s.
    rate: Vector3<f32>,
    /// Sensor clock of the last sample, seconds, unwrapped.
    t_sensor: f64,
    last_ticks: Option<u16>,
    /// Smallest (host − sensor) clock offset seen: the least-delayed packet.
    offset: Option<f64>,
    recentred: bool,
}

impl ExternalTracker {
    /// New tracker; the first sample recentres it.
    pub fn new(frame: SensorFrame, cfg: ExternalConfig) -> Self {
        Self {
            frame,
            cfg,
            mount: UnitQuaternion::identity(),
            yaw_zero: UnitQuaternion::identity(),
            raw: None,
            rate: Vector3::zeros(),
            t_sensor: 0.0,
            last_ticks: None,
            offset: None,
            recentred: false,
        }
    }

    /// True once a sample has arrived.
    pub fn has_pose(&self) -> bool {
        self.raw.is_some()
    }

    /// Feed one sample. `ticks_ms` is the sensor's own 16-bit millisecond
    /// clock (it wraps); `host_s` is when the packet carrying it arrived;
    /// `q_xyzw` the sensor quaternion. Samples of one packet share `host_s`.
    pub fn feed(&mut self, ticks_ms: u16, host_s: f64, q_xyzw: [f32; 4]) -> Pose {
        let q = Quaternion::new(q_xyzw[3], q_xyzw[0], q_xyzw[1], q_xyzw[2]);
        if q.norm() < 1e-3 {
            return self.pose_at(host_s); // not a rotation: ignore
        }
        let now = self.frame.to_rub(&UnitQuaternion::new_normalize(q));
        let dt = match self.last_ticks {
            Some(prev) => f64::from(ticks_ms.wrapping_sub(prev)) / 1000.0,
            None => 0.0,
        };
        self.last_ticks = Some(ticks_ms);
        self.t_sensor += dt;
        // The packet that waited least gives the clock offset; let it creep
        // up slowly so a drifting sensor clock cannot pin it forever.
        let seen = host_s - self.t_sensor;
        self.offset = Some(match self.offset {
            Some(o) => (o + 0.0005).min(seen),
            None => seen,
        });
        if let Some(prev) = self.raw {
            if dt > 0.0 && dt < 0.5 {
                let measured = (prev.inverse() * now).scaled_axis() / dt as f32;
                // `measured` is in the sensor-mounted body frame; the head
                // frame differs by the mount tilt.
                let head = self.mount * measured;
                let a = self.cfg.rate_smoothing.clamp(0.0, 1.0);
                self.rate = self.rate * (1.0 - a) + head * a;
            } else {
                self.rate = Vector3::zeros(); // gap: do not extrapolate across it
            }
        }
        self.raw = Some(now);
        if !self.recentred {
            self.recenter();
        }
        self.pose_at(host_s)
    }

    /// The wearer is looking straight ahead with a level head *now*: that
    /// gaze becomes yaw 0, pitch 0, roll 0. Removes both the arbitrary world
    /// heading and the way the headset happens to sit on the head.
    pub fn recenter(&mut self) {
        let Some(raw) = self.raw else { return };
        let yaw = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), world_yaw(&raw));
        // raw = yaw · tilt, with `tilt` the sensor's attitude on a level head.
        self.mount = yaw.inverse() * raw;
        self.yaw_zero = yaw.inverse();
        self.recentred = true;
    }

    fn corrected(&self, raw: &UnitQuaternion<f32>) -> UnitQuaternion<f32> {
        self.yaw_zero * raw * self.mount.inverse()
    }

    /// Pose for a frame shown at host time `host_s` (+ `base_latency_s`),
    /// extrapolated from the newest sample by the estimated head rate.
    pub fn pose_at(&self, host_s: f64) -> Pose {
        let Some(raw) = self.raw else {
            return Pose {
                confidence: 0.0,
                ..Pose::identity()
            };
        };
        let age = (host_s - (self.t_sensor + self.offset.unwrap_or(host_s - self.t_sensor))).max(0.0) as f32;
        let horizon = (age + self.cfg.base_latency_s).min(self.cfg.max_prediction_s);
        let rate = if self.rate.norm() < self.cfg.rate_deadband {
            Vector3::zeros()
        } else {
            self.rate
        };
        Pose {
            t_us: (host_s.max(0.0) * 1e6) as u64,
            orientation: integrate(&self.corrected(&raw), &rate, horizon),
            angular_velocity: rate,
            // A link that has gone quiet is not to be trusted.
            confidence: if age > 0.5 { 0.3 } else { 1.0 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{world_pitch, world_yaw};

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    /// A head pose (RUB) as a Bose sensor sitting at `tilt` on the head would report it.
    fn bose_reading(head: UnitQuaternion<f32>, tilt: UnitQuaternion<f32>, heading: f32) -> [f32; 4] {
        let f = SensorFrame::bose_ar();
        let world_spin = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), heading);
        let rub = world_spin * head * tilt; // what to_rub() must return
        let q = f.world_to_rub.inverse() * rub * f.body_to_rub;
        [q.i, q.j, q.k, q.w]
    }

    fn tracker() -> ExternalTracker {
        ExternalTracker::new(SensorFrame::bose_ar(), ExternalConfig::default())
    }

    #[test]
    fn first_sample_is_straight_ahead_whatever_the_mount() {
        let tilt = UnitQuaternion::from_euler_angles(deg(18.0), 0.0, deg(4.0));
        let mut t = tracker();
        let p = t.feed(100, 5.0, bose_reading(UnitQuaternion::identity(), tilt, 2.1));
        assert!(p.orientation.angle() < 1e-3, "angle {}", p.orientation.angle());
        assert_eq!(p.confidence, 1.0);
    }

    #[test]
    fn turning_left_is_positive_yaw_and_looking_up_positive_pitch() {
        let tilt = UnitQuaternion::from_euler_angles(deg(18.0), 0.0, deg(-3.0));
        let mut t = tracker();
        t.feed(0, 0.0, bose_reading(UnitQuaternion::identity(), tilt, -0.7));
        let left = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), deg(30.0));
        let p = t.feed(1000, 1.0, bose_reading(left, tilt, -0.7));
        assert!((world_yaw(&p.orientation) - deg(30.0)).abs() < 1e-3);
        assert!(world_pitch(&p.orientation).abs() < 1e-3);
        let up = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), deg(20.0));
        let p = t.feed(2000, 2.0, bose_reading(up, tilt, -0.7));
        // A sensor that sits both pitched and rolled leaves "straight ahead"
        // known only to within about a degree from one pose; allow for it.
        assert!((world_pitch(&p.orientation) - deg(20.0)).abs() < deg(0.5));
        assert!(world_yaw(&p.orientation).abs() < deg(0.5));
    }

    /// The three poses recorded from the real headset (hardware-log 2026-11-14):
    /// "left" must come out as a left turn, "up" as looking up.
    #[test]
    fn recorded_bose_marks_have_the_right_signs() {
        let mut t = tracker();
        t.feed(0, 0.0, [0.894, 0.429, -0.002, 0.130]);
        let left = t.feed(1000, 1.0, [0.579, 0.804, 0.078, 0.115]);
        assert!(
            world_yaw(&left.orientation) > deg(40.0),
            "yaw {}",
            world_yaw(&left.orientation).to_degrees()
        );
        let up = t.feed(2000, 2.0, [0.807, 0.571, -0.097, -0.115]);
        assert!(
            world_pitch(&up.orientation) > deg(15.0),
            "pitch {}",
            world_pitch(&up.orientation).to_degrees()
        );
    }

    #[test]
    fn recenter_makes_the_current_gaze_zero() {
        let mut t = tracker();
        t.feed(
            0,
            0.0,
            bose_reading(UnitQuaternion::identity(), UnitQuaternion::identity(), 0.0),
        );
        let left = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), deg(50.0));
        t.feed(1000, 1.0, bose_reading(left, UnitQuaternion::identity(), 0.0));
        t.recenter();
        assert!(t.pose_at(1.0).orientation.angle() < 1e-3);
    }

    #[test]
    fn still_head_stays_still() {
        let mut t = tracker();
        let q = bose_reading(UnitQuaternion::identity(), UnitQuaternion::identity(), 0.3);
        for i in 0..200u16 {
            t.feed(i * 20, f64::from(i) * 0.02, q);
        }
        assert!(t.pose_at(4.05).orientation.angle() < 1e-4);
    }

    #[test]
    fn prediction_moves_ahead_during_a_turn_and_is_capped() {
        let mut t = tracker();
        // 60°/s to the left, 50 Hz.
        for i in 0..50u16 {
            let head = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), deg(60.0) * f32::from(i) * 0.02);
            t.feed(
                i * 20,
                f64::from(i) * 0.02,
                bose_reading(head, UnitQuaternion::identity(), 0.0),
            );
        }
        let at_sample = deg(60.0) * 49.0 * 0.02;
        let now = world_yaw(&t.pose_at(0.98).orientation);
        let later = world_yaw(&t.pose_at(1.02).orientation);
        assert!(now > at_sample, "prediction must lead the last sample");
        assert!(later > now);
        // A link that stalls for seconds holds the pose instead of spinning on.
        let stalled = world_yaw(&t.pose_at(9.0).orientation);
        assert!(stalled - at_sample <= deg(60.0) * ExternalConfig::default().max_prediction_s + 1e-3);
        assert!(t.pose_at(9.0).confidence < 1.0);
    }

    #[test]
    fn samples_packed_in_one_packet_use_the_sensor_clock() {
        // Three samples 20 ms apart arriving together must give the true rate,
        // not an infinite one from the shared arrival time.
        let mut t = ExternalTracker::new(
            SensorFrame::bose_ar(),
            ExternalConfig {
                rate_smoothing: 1.0,
                ..ExternalConfig::default()
            },
        );
        for i in 0..3u16 {
            let head = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), deg(1.0) * f32::from(i));
            t.feed(i * 20, 10.0, bose_reading(head, UnitQuaternion::identity(), 0.0));
        }
        let rate = t.pose_at(10.0).angular_velocity;
        assert!((rate.y - deg(50.0)).abs() < deg(1.0), "rate {:?}", rate);
    }

    #[test]
    fn sensor_clock_wraps() {
        let mut t = tracker();
        let q = bose_reading(UnitQuaternion::identity(), UnitQuaternion::identity(), 0.0);
        t.feed(65_530, 1.0, q);
        t.feed(14, 1.02, q); // 20 ms later, across the wrap
        assert!((t.t_sensor - 0.02).abs() < 1e-9);
    }

    #[test]
    fn a_zero_quaternion_is_ignored() {
        let mut t = tracker();
        let p = t.feed(0, 0.0, [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(p.confidence, 0.0);
        assert!(!t.has_pose());
    }
}
