//! `perch-space` — the 3DOF "world" the windows live in.
//!
//! This crate is pure math + state, no I/O, so it can be unit-tested on CI
//! without glasses and reused by every platform (the Android host app links
//! it through `perch-ffi`).
//!
//! Responsibilities:
//! * [`Workspace`] — the set of [`SpatialWindow`]s, focus, presets, layouts.
//! * [`Projector`] — given the head pose and the display's optics, where does
//!   each window land on the glasses' 1920×1080 (or 1200) panel?
//! * [`SmoothFollower`] — the "lazy follow" anchor behaviour.
//! * Gaze hit-testing for look-to-focus.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use nalgebra::{Matrix3, Point2, Point3, UnitQuaternion, Vector3};
use perch_proto::{Anchor, Layout, Placement, Pose, Source, SpatialWindow, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Errors from workspace operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpaceError {
    /// No window with that id.
    #[error("no such window")]
    NoSuchWindow,
    /// No layout with that name.
    #[error("no such layout: {0}")]
    NoSuchLayout(String),
    /// Unknown preset.
    #[error("unknown preset: {0}")]
    UnknownPreset(String),
}

// ---------------------------------------------------------------------------
// Display optics
// ---------------------------------------------------------------------------

/// Optical description of the glasses panel for one eye (or the mono image).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayOptics {
    /// Panel width in pixels of the image we render (1920 for mono).
    pub width_px: u32,
    /// Panel height in pixels (1080 or 1200).
    pub height_px: u32,
    /// Horizontal field of view in radians.
    pub fov_h: f32,
    /// Forward tilt of the display relative to the IMU, radians (the glasses'
    /// screens look slightly downward). Rokid Max ≈ 0.07 rad per `ar-drivers`.
    pub tilt: f32,
}

impl DisplayOptics {
    /// Rokid Max, 1920×1080 mono. FOV 23° horizontal is the value measured by
    /// `ar-drivers`; the marketing number is 50° diagonal, which is larger
    /// because it is diagonal on a 16:9 frame.
    pub fn rokid_max_1080p() -> Self {
        Self {
            width_px: 1920,
            height_px: 1080,
            fov_h: 46f32.to_radians(),
            tilt: 0.07,
        }
    }
    /// Rokid Max, 1920×1200 mode.
    pub fn rokid_max_1200p() -> Self {
        Self {
            height_px: 1200,
            ..Self::rokid_max_1080p()
        }
    }
    /// Focal length in pixels (pinhole).
    pub fn focal_px(&self) -> f32 {
        (self.width_px as f32 / 2.0) / (self.fov_h / 2.0).tan()
    }
    /// Vertical FOV implied by the aspect ratio.
    pub fn fov_v(&self) -> f32 {
        2.0 * ((self.height_px as f32 / 2.0) / self.focal_px()).atan()
    }
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// A window projected onto the panel: four corners in pixels, clockwise from
/// top-left, plus depth for sorting. Corners may fall outside the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenQuad {
    /// Window id.
    pub id: WindowId,
    /// Corners in panel pixels: TL, TR, BR, BL.
    pub corners: [Point2<f32>; 4],
    /// Mean depth (metres) for painter's ordering.
    pub depth: f32,
    /// Z-order from the window.
    pub z: i32,
    /// Opacity from the window.
    pub opacity: f32,
    /// Whether any part is on screen.
    pub on_screen: bool,
}

/// Projects world-placed windows into panel pixels.
#[derive(Debug, Clone, Copy)]
pub struct Projector {
    /// Optics.
    pub optics: DisplayOptics,
}

impl Projector {
    /// New projector.
    pub fn new(optics: DisplayOptics) -> Self {
        Self { optics }
    }

    /// World-space corners of a window: a rectangle centred on the direction
    /// `(yaw, pitch)` at `distance`, facing the head origin, rolled by `roll`.
    pub fn world_corners(p: &Placement) -> [Point3<f32>; 4] {
        // Direction basis: start looking down -Z, rotate by pitch about X then yaw about Y.
        let r = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), p.yaw)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), p.pitch)
            * UnitQuaternion::from_axis_angle(&Vector3::z_axis(), p.roll);
        let centre = r * Vector3::new(0.0, 0.0, -p.distance);
        let right = r * Vector3::x() * (p.width_m / 2.0);
        let up = r * Vector3::y() * (p.height_m / 2.0);
        [
            Point3::from(centre - right + up), // TL
            Point3::from(centre + right + up), // TR
            Point3::from(centre + right - up), // BR
            Point3::from(centre - right - up), // BL
        ]
    }

    /// Project one window given the head pose. Windows behind the head return
    /// `None`.
    pub fn project(&self, w: &SpatialWindow, pose: &Pose) -> Option<ScreenQuad> {
        let placement = match w.anchor {
            // Head-locked: ignore pose entirely.
            Anchor::Head => w.placement,
            Anchor::World | Anchor::SmoothFollow => w.placement,
        };
        let corners = Self::world_corners(&placement);
        // World → head: inverse of head→world. Then apply display tilt.
        let head_from_world = if w.anchor == Anchor::Head {
            UnitQuaternion::identity()
        } else {
            pose.orientation.inverse()
        };
        let tilt = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -self.optics.tilt);
        let f = self.optics.focal_px();
        let cx = self.optics.width_px as f32 / 2.0;
        let cy = self.optics.height_px as f32 / 2.0;
        let mut out = [Point2::origin(); 4];
        let mut depth = 0.0;
        for (i, c) in corners.iter().enumerate() {
            let h = tilt * (head_from_world * c.coords);
            // Camera looks down -Z. Reject if behind.
            let zc = -h.z;
            if zc <= 0.01 {
                return None;
            }
            out[i] = Point2::new(cx + f * h.x / zc, cy - f * h.y / zc);
            depth += zc;
        }
        let on_screen = out.iter().any(|p| {
            p.x >= 0.0 && p.x <= self.optics.width_px as f32 && p.y >= 0.0 && p.y <= self.optics.height_px as f32
        }) || quad_contains(&out, Point2::new(cx, cy));
        Some(ScreenQuad {
            id: w.id,
            corners: out,
            depth: depth / 4.0,
            z: w.z,
            opacity: w.opacity,
            on_screen,
        })
    }

    /// Project all visible windows, back-to-front (painter's order, then z).
    pub fn project_all(&self, ws: &Workspace, pose: &Pose) -> Vec<ScreenQuad> {
        let mut v: Vec<ScreenQuad> = ws
            .windows
            .iter()
            .filter(|w| w.visible)
            .filter_map(|w| self.project(w, pose))
            .collect();
        v.sort_by(|a, b| {
            a.z.cmp(&b.z)
                .then(b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal))
        });
        v
    }

    /// 3×3 homography mapping unit square (texture coords, origin top-left)
    /// to panel pixels for a quad. Compositors can upload this directly.
    pub fn homography(q: &ScreenQuad) -> Matrix3<f32> {
        // Standard 4-point DLT for the square (0,0),(1,0),(1,1),(0,1) → corners.
        let [p0, p1, p2, p3] = q.corners;
        let (x0, y0, x1, y1, x2, y2, x3, y3) = (p0.x, p0.y, p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
        let dx1 = x1 - x2;
        let dx2 = x3 - x2;
        let dx3 = x0 - x1 + x2 - x3;
        let dy1 = y1 - y2;
        let dy2 = y3 - y2;
        let dy3 = y0 - y1 + y2 - y3;
        let det = dx1 * dy2 - dx2 * dy1;
        let (g, h) = if det.abs() < 1e-9 {
            (0.0, 0.0)
        } else {
            ((dx3 * dy2 - dx2 * dy3) / det, (dx1 * dy3 - dx3 * dy1) / det)
        };
        let a = x1 - x0 + g * x1;
        let b = x3 - x0 + h * x3;
        let c = x0;
        let d = y1 - y0 + g * y1;
        let e = y3 - y0 + h * y3;
        let f = y0;
        Matrix3::new(a, b, c, d, e, f, g, h, 1.0)
    }
}

/// Point-in-convex-quad test (any winding).
pub fn quad_contains(q: &[Point2<f32>; 4], p: Point2<f32>) -> bool {
    let mut sign = 0i8;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let s = if cross > 0.0 {
            1
        } else if cross < 0.0 {
            -1
        } else {
            0
        };
        if s != 0 {
            if sign == 0 {
                sign = s;
            } else if sign != s {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Smooth follow
// ---------------------------------------------------------------------------

/// Per-window state for [`Anchor::SmoothFollow`]: the window stays put until
/// the gaze leaves a dead zone, then glides toward the gaze direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SmoothFollower {
    /// Current yaw of the window (what the renderer should use).
    pub yaw: f32,
    /// Current pitch.
    pub pitch: f32,
    /// Dead-zone half-angle, radians. Beyond this the window starts moving.
    pub threshold: f32,
    /// Exponential smoothing time constant, seconds.
    pub tau: f32,
    /// Whether currently gliding.
    pub moving: bool,
}

impl SmoothFollower {
    /// New follower starting at a placement.
    pub fn new(p: &Placement) -> Self {
        Self {
            yaw: p.yaw,
            pitch: p.pitch,
            threshold: 20f32.to_radians(),
            tau: 0.35,
            moving: false,
        }
    }

    /// Advance by `dt` seconds toward the gaze direction `(gaze_yaw, gaze_pitch)`.
    /// Returns the placement to render with.
    pub fn step(&mut self, base: &Placement, gaze_yaw: f32, gaze_pitch: f32, dt: f32) -> Placement {
        let dy = wrap_pi(gaze_yaw - self.yaw);
        let dp = gaze_pitch - self.pitch;
        let dist = (dy * dy + dp * dp).sqrt();
        if !self.moving && dist > self.threshold {
            self.moving = true;
        }
        if self.moving {
            let alpha = 1.0 - (-dt / self.tau).exp();
            self.yaw = wrap_pi(self.yaw + dy * alpha);
            self.pitch += dp * alpha;
            if dist < 0.5f32.to_radians() {
                self.moving = false;
            }
        }
        Placement {
            yaw: self.yaw,
            pitch: self.pitch,
            ..*base
        }
    }
}

/// Wrap an angle to (-π, π].
pub fn wrap_pi(a: f32) -> f32 {
    let two_pi = std::f32::consts::TAU;
    let mut a = (a + std::f32::consts::PI) % two_pi;
    if a < 0.0 {
        a += two_pi;
    }
    a - std::f32::consts::PI
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

/// The live set of spatial windows plus saved layouts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workspace {
    /// Windows, in creation order.
    pub windows: Vec<SpatialWindow>,
    /// Focused window.
    pub focused: Option<WindowId>,
    /// Saved layouts by name.
    pub layouts: HashMap<String, Layout>,
    /// Smooth-follow state, keyed by window id (not persisted).
    #[serde(skip)]
    pub followers: HashMap<WindowId, SmoothFollower>,
}

impl Workspace {
    /// Empty workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Find a window.
    pub fn get(&self, id: WindowId) -> Option<&SpatialWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn get_mut(&mut self, id: WindowId) -> Result<&mut SpatialWindow, SpaceError> {
        self.windows
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or(SpaceError::NoSuchWindow)
    }

    /// Insert or replace a window. The first window inserted becomes focused.
    pub fn upsert(&mut self, w: SpatialWindow) {
        if let Some(existing) = self.windows.iter_mut().find(|x| x.id == w.id) {
            *existing = w;
        } else {
            self.windows.push(w);
        }
        if self.focused.is_none() {
            self.focused = self.windows.first().map(|w| w.id);
        }
        self.sync_focus_flags();
    }

    /// Remove a window.
    pub fn remove(&mut self, id: WindowId) -> Result<(), SpaceError> {
        let before = self.windows.len();
        self.windows.retain(|w| w.id != id);
        if self.windows.len() == before {
            return Err(SpaceError::NoSuchWindow);
        }
        self.followers.remove(&id);
        if self.focused == Some(id) {
            self.focused = self.windows.iter().find(|w| w.visible).map(|w| w.id);
        }
        self.sync_focus_flags();
        Ok(())
    }

    /// Move/resize.
    pub fn place(&mut self, id: WindowId, p: Placement) -> Result<(), SpaceError> {
        self.get_mut(id)?.placement = p;
        if let Some(f) = self.followers.get_mut(&id) {
            f.yaw = p.yaw;
            f.pitch = p.pitch;
        }
        Ok(())
    }

    /// Pin the window at the current gaze direction (keeps size/distance).
    pub fn pin_here(&mut self, id: WindowId, pose: &Pose) -> Result<(), SpaceError> {
        let (yaw, pitch) = gaze_angles(pose);
        let w = self.get_mut(id)?;
        w.placement.yaw = yaw;
        w.placement.pitch = pitch;
        w.anchor = Anchor::World;
        self.followers.remove(&id);
        Ok(())
    }

    /// Change the anchor behaviour of a window.
    pub fn set_anchor(&mut self, id: WindowId, anchor: Anchor) -> Result<(), SpaceError> {
        let w = self.get_mut(id)?;
        w.anchor = anchor;
        let p = w.placement;
        match anchor {
            Anchor::SmoothFollow => {
                self.followers.insert(id, SmoothFollower::new(&p));
            }
            _ => {
                self.followers.remove(&id);
            }
        }
        Ok(())
    }

    /// Focus a window.
    pub fn focus(&mut self, id: WindowId) -> Result<(), SpaceError> {
        self.get_mut(id)?;
        self.focused = Some(id);
        self.sync_focus_flags();
        Ok(())
    }

    /// Cycle focus through visible windows.
    pub fn focus_next(&mut self) -> Option<WindowId> {
        let vis: Vec<WindowId> = self.windows.iter().filter(|w| w.visible).map(|w| w.id).collect();
        if vis.is_empty() {
            self.focused = None;
            return None;
        }
        let next = match self.focused.and_then(|f| vis.iter().position(|&id| id == f)) {
            Some(i) => vis[(i + 1) % vis.len()],
            None => vis[0],
        };
        self.focused = Some(next);
        self.sync_focus_flags();
        Some(next)
    }

    fn sync_focus_flags(&mut self) {
        for w in &mut self.windows {
            w.focused = Some(w.id) == self.focused;
        }
    }

    /// The window the gaze is on (closest to centre of view among hits), for
    /// look-to-focus.
    pub fn gaze_hit(&self, projector: &Projector, pose: &Pose) -> Option<WindowId> {
        let cx = projector.optics.width_px as f32 / 2.0;
        let cy = projector.optics.height_px as f32 / 2.0;
        let centre = Point2::new(cx, cy);
        projector
            .project_all(self, pose)
            .into_iter()
            .rev() // topmost first
            .find(|q| quad_contains(&q.corners, centre))
            .map(|q| q.id)
    }

    /// Advance smooth-follow windows. Call once per frame with the current pose.
    /// Returns the placements to render with for *all* windows.
    pub fn tick(&mut self, pose: &Pose, dt: f32) -> Vec<(WindowId, Placement)> {
        let (gy, gp) = gaze_angles(pose);
        let mut out = Vec::with_capacity(self.windows.len());
        for w in &self.windows {
            let p = match w.anchor {
                Anchor::SmoothFollow => {
                    let f = self
                        .followers
                        .entry(w.id)
                        .or_insert_with(|| SmoothFollower::new(&w.placement));
                    f.step(&w.placement, gy, gp, dt)
                }
                _ => w.placement,
            };
            out.push((w.id, p));
        }
        out
    }

    /// Snapshot current windows under a name.
    pub fn save_layout(&mut self, name: &str) {
        self.layouts.insert(
            name.to_string(),
            Layout {
                name: name.to_string(),
                windows: self.windows.clone(),
            },
        );
    }

    /// Restore a saved layout. Window ids are preserved.
    pub fn load_layout(&mut self, name: &str) -> Result<(), SpaceError> {
        let l = self
            .layouts
            .get(name)
            .cloned()
            .ok_or_else(|| SpaceError::NoSuchLayout(name.to_string()))?;
        self.windows = l.windows;
        self.followers.clear();
        self.focused = self
            .windows
            .iter()
            .find(|w| w.focused)
            .or(self.windows.first())
            .map(|w| w.id);
        self.sync_focus_flags();
        Ok(())
    }

    /// Built-in presets. They re-place *existing* windows (by order) and fall
    /// back to creating virtual displays for `peer` when there aren't enough.
    ///
    /// * `single`  — one 16:9 window dead ahead, 2 m.
    /// * `coding`  — centre editor, left terminal (angled in), right docs.
    /// * `gaming`  — one large head-anchored window (no tracking lag, max FOV).
    /// * `couch`   — one window pitched down 15° so you can recline.
    pub fn apply_preset(&mut self, name: &str, peer: Uuid) -> Result<(), SpaceError> {
        let specs: Vec<(Placement, Anchor)> = match name {
            "single" => vec![(place(0.0, 0.0, 2.0, 1.78, 1.0), Anchor::World)],
            "coding" => vec![
                (place(0.0, 0.0, 2.0, 1.6, 0.9), Anchor::World),
                (place(0.55, 0.0, 2.1, 1.1, 0.9), Anchor::World),
                (place(-0.55, 0.0, 2.1, 1.1, 0.9), Anchor::World),
            ],
            "gaming" => vec![(place(0.0, 0.0, 1.8, 1.78, 1.0), Anchor::Head)],
            "couch" => vec![(place(0.0, -0.26, 2.0, 1.78, 1.0), Anchor::SmoothFollow)],
            other => return Err(SpaceError::UnknownPreset(other.to_string())),
        };
        for (i, (p, a)) in specs.iter().enumerate() {
            if let Some(w) = self.windows.get_mut(i) {
                w.placement = *p;
                w.anchor = *a;
                w.visible = true;
                w.z = i as i32;
            } else {
                let id = Uuid::new_v4();
                self.windows.push(SpatialWindow {
                    id,
                    source: Source::Virtual {
                        peer,
                        width: 1920,
                        height: (1920.0 * p.height_m / p.width_m) as u32,
                    },
                    placement: *p,
                    anchor: *a,
                    z: i as i32,
                    opacity: 1.0,
                    visible: true,
                    focused: false,
                });
            }
        }
        for w in self.windows.iter_mut().skip(specs.len()) {
            w.visible = false;
        }
        self.followers.clear();
        self.focused = self.windows.first().map(|w| w.id);
        self.sync_focus_flags();
        Ok(())
    }

    /// Names of the built-in presets.
    pub fn preset_names() -> &'static [&'static str] {
        &["single", "coding", "gaming", "couch"]
    }

    /// Serialise to JSON for persistence.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("workspace is serialisable")
    }
    /// Deserialise.
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// Gaze direction `(yaw, pitch)` of a pose.
pub fn gaze_angles(pose: &Pose) -> (f32, f32) {
    let fwd = pose.orientation * -Vector3::z();
    let yaw = (-fwd.x).atan2(-fwd.z);
    let pitch = fwd.y.clamp(-1.0, 1.0).asin();
    (yaw, pitch)
}

fn place(yaw: f32, pitch: f32, distance: f32, width_m: f32, height_m: f32) -> Placement {
    Placement {
        yaw,
        pitch,
        distance,
        roll: 0.0,
        width_m,
        height_m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(yaw: f32, pitch: f32) -> SpatialWindow {
        SpatialWindow {
            id: Uuid::new_v4(),
            source: Source::Virtual {
                peer: Uuid::nil(),
                width: 1920,
                height: 1080,
            },
            placement: place(yaw, pitch, 2.0, 1.6, 0.9),
            anchor: Anchor::World,
            z: 0,
            opacity: 1.0,
            visible: true,
            focused: false,
        }
    }

    fn pose_yaw(yaw: f32) -> Pose {
        Pose {
            orientation: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw),
            ..Pose::identity()
        }
    }

    #[test]
    fn window_ahead_is_centred() {
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let q = pr.project(&win(0.0, 0.0), &Pose::identity()).unwrap();
        let cx: f32 = q.corners.iter().map(|p| p.x).sum::<f32>() / 4.0;
        let cy: f32 = q.corners.iter().map(|p| p.y).sum::<f32>() / 4.0;
        assert!((cx - 960.0).abs() < 1e-2 && (cy - 540.0).abs() < 1e-2);
        assert!(q.on_screen);
        // TL must be above-left of BR
        assert!(q.corners[0].x < q.corners[2].x && q.corners[0].y < q.corners[2].y);
    }

    #[test]
    fn turning_head_left_moves_window_right() {
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let w = win(0.0, 0.0);
        let q0 = pr.project(&w, &Pose::identity()).unwrap();
        let q1 = pr.project(&w, &pose_yaw(0.1)).unwrap();
        assert!(q1.corners[0].x > q0.corners[0].x);
    }

    #[test]
    fn world_window_stays_put_when_looking_at_it() {
        // Window pinned 30° left; look 30° left → centred.
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let w = win(0.5, 0.0);
        let q = pr.project(&w, &pose_yaw(0.5)).unwrap();
        let cx: f32 = q.corners.iter().map(|p| p.x).sum::<f32>() / 4.0;
        assert!((cx - 960.0).abs() < 1e-2);
    }

    #[test]
    fn head_anchored_ignores_pose() {
        let pr = Projector::new(DisplayOptics::rokid_max_1080p());
        let mut w = win(0.0, 0.0);
        w.anchor = Anchor::Head;
        let a = pr.project(&w, &Pose::identity()).unwrap();
        let b = pr.project(&w, &pose_yaw(1.0)).unwrap();
        assert_eq!(a.corners, b.corners);
    }

    #[test]
    fn behind_head_is_culled() {
        let pr = Projector::new(DisplayOptics::rokid_max_1080p());
        assert!(pr.project(&win(std::f32::consts::PI, 0.0), &Pose::identity()).is_none());
    }

    #[test]
    fn gaze_hit_picks_topmost_window_under_centre() {
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let mut ws = Workspace::new();
        let mut a = win(0.0, 0.0);
        a.z = 0;
        let mut b = win(0.05, 0.0);
        b.z = 1;
        ws.upsert(a.clone());
        ws.upsert(b.clone());
        assert_eq!(ws.gaze_hit(&pr, &Pose::identity()), Some(b.id));
        assert_eq!(ws.gaze_hit(&pr, &pose_yaw(1.5)), None);
    }

    #[test]
    fn pin_here_moves_to_gaze() {
        let mut ws = Workspace::new();
        let w = win(0.0, 0.0);
        ws.upsert(w.clone());
        ws.pin_here(w.id, &pose_yaw(0.7)).unwrap();
        assert!((ws.get(w.id).unwrap().placement.yaw - 0.7).abs() < 1e-5);
    }

    #[test]
    fn focus_cycles_visible_only() {
        let mut ws = Workspace::new();
        let a = win(0.0, 0.0);
        let mut b = win(0.1, 0.0);
        b.visible = false;
        let c = win(0.2, 0.0);
        ws.upsert(a.clone());
        ws.upsert(b.clone());
        ws.upsert(c.clone());
        assert_eq!(ws.focused, Some(a.id));
        assert_eq!(ws.focus_next(), Some(c.id));
        assert_eq!(ws.focus_next(), Some(a.id));
        assert!(ws.get(a.id).unwrap().focused);
        assert!(!ws.get(c.id).unwrap().focused);
    }

    #[test]
    fn smooth_follow_waits_then_glides() {
        let base = place(0.0, 0.0, 2.0, 1.6, 0.9);
        let mut f = SmoothFollower::new(&base);
        // Small gaze change inside the dead zone: no movement.
        let p = f.step(&base, 0.1, 0.0, 0.016);
        assert_eq!(p.yaw, 0.0);
        // Big gaze change: starts moving and converges.
        let mut p = base;
        for _ in 0..600 {
            p = f.step(&base, 0.8, 0.0, 0.016);
        }
        assert!((p.yaw - 0.8).abs() < 0.01, "yaw = {}", p.yaw);
    }

    #[test]
    fn presets_and_layouts_roundtrip() {
        let mut ws = Workspace::new();
        ws.apply_preset("coding", Uuid::nil()).unwrap();
        assert_eq!(ws.windows.len(), 3);
        ws.save_layout("work");
        ws.apply_preset("single", Uuid::nil()).unwrap();
        assert_eq!(ws.windows.iter().filter(|w| w.visible).count(), 1);
        let json = ws.to_json();
        let mut back = Workspace::from_json(&json).unwrap();
        back.load_layout("work").unwrap();
        assert_eq!(back.windows.iter().filter(|w| w.visible).count(), 3);
        assert_eq!(
            ws.apply_preset("nope", Uuid::nil()),
            Err(SpaceError::UnknownPreset("nope".into()))
        );
    }

    #[test]
    fn homography_maps_unit_square_to_corners() {
        let pr = Projector::new(DisplayOptics::rokid_max_1080p());
        let q = pr.project(&win(0.2, -0.1), &Pose::identity()).unwrap();
        let h = Projector::homography(&q);
        let map = |u: f32, v: f32| {
            let r = h * Vector3::new(u, v, 1.0);
            Point2::new(r.x / r.z, r.y / r.z)
        };
        let tol = 1e-2;
        assert!((map(0.0, 0.0) - q.corners[0]).norm() < tol);
        assert!((map(1.0, 0.0) - q.corners[1]).norm() < tol);
        assert!((map(1.0, 1.0) - q.corners[2]).norm() < tol);
        assert!((map(0.0, 1.0) - q.corners[3]).norm() < tol);
    }

    #[test]
    fn wrap() {
        assert!((wrap_pi(3.0 * std::f32::consts::PI).abs() - std::f32::consts::PI).abs() < 1e-5);
        assert!((wrap_pi(0.3) - 0.3).abs() < 1e-6);
        assert!((wrap_pi(-std::f32::consts::PI * 1.5) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }
}
