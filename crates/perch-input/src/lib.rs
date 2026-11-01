//! `perch-input` — one keyboard and mouse for every window in the glasses,
//! regardless of which machine the window lives on.
//!
//! The model is Barrier/Synergy's, but the "screen edges" are *spatial*: the
//! pointer lives inside the focused [`SpatialWindow`]; when it is pushed past
//! that window's edge toward a neighbouring window, focus (and therefore the
//! destination of all subsequent input) hops to that window's peer and the
//! pointer appears at the matching edge. Keyboard input always goes to the
//! focused window's peer.
//!
//! ```text
//!  local devices ──► InputSource ──► Router ──► local InputSink   (same peer)
//!  phone touchpad ─► net ──────────► Router ──► net ──► remote InputSink
//! ```
//!
//! Pure parts (router, edge switcher, keymaps) live here and are tested on
//! CI. OS backends are feature-gated modules.

#![warn(missing_docs)]

use nalgebra::Point2;
use perch_proto::{InputEvent, SpatialWindow, WindowId};
use perch_space::{Projector, ScreenQuad};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod keymap;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    /// Backend unavailable (missing permission / device).
    #[error("input backend unavailable: {0}")]
    Unavailable(String),
    /// I/O.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Where `MouseMoveAbs` (window-normalised 0..1) lands on the target OS's
/// screen: the window's rectangle and the whole desktop's extent, both in
/// the OS's logical pixels. Without it a sink has to fall back to relative
/// motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsTarget {
    /// Window/monitor rectangle on the desktop: x, y, width, height.
    pub rect: (f32, f32, f32, f32),
    /// Desktop extent (all outputs): width, height.
    pub bounds: (f32, f32),
}

/// Something that can inject events into an OS (local uinput, or a proxy that
/// forwards to a remote peer).
pub trait InputSink: Send {
    /// Inject one event.
    fn inject(&mut self, ev: InputEvent) -> Result<(), InputError>;
    /// Flush (uinput SYN_REPORT etc.).
    fn flush(&mut self) -> Result<(), InputError> {
        Ok(())
    }
    /// Tell the sink where subsequent `MouseMoveAbs` events map to.
    fn set_abs_target(&mut self, _target: Option<AbsTarget>) {}
}

/// Something that produces events from physical devices.
pub trait InputSource: Send {
    /// Block for the next event.
    fn next(&mut self) -> Result<InputEvent, InputError>;
    /// Grab devices exclusively (so the local OS no longer sees them) when
    /// routing to a remote peer; release otherwise.
    fn set_grabbed(&mut self, grabbed: bool) -> Result<(), InputError>;
}

// ---------------------------------------------------------------------------
// Pointer state inside the focused window
// ---------------------------------------------------------------------------

/// Which edge of a window the pointer crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Top.
    Top,
    /// Bottom.
    Bottom,
}

/// Router decision for a batch of input.
#[derive(Debug, Clone, PartialEq)]
pub enum Routed {
    /// Deliver `events` to `peer` (the owner of `window`).
    Deliver {
        /// Destination peer.
        peer: Uuid,
        /// Window the pointer is in.
        window: WindowId,
        /// Events, with `MouseMove` converted to absolute window-normalised
        /// coordinates (`MouseMoveAbs`) so the receiver doesn't need our DPI.
        events: Vec<InputEvent>,
    },
    /// Focus hopped to a neighbouring window; caller should emit a
    /// `FocusChanged` event and re-run with the same batch.
    Switched {
        /// New window.
        to: WindowId,
        /// Edge of the *new* window where the pointer entered.
        entered_from: Edge,
    },
    /// No focused window; events dropped.
    NoFocus,
}

/// Routes input to the focused window's peer and performs edge switching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Router {
    /// Pointer position normalised 0..1 inside the focused window.
    pub pointer: Point2<f32>,
    /// Pointer speed: how many normalised window widths one logical pixel is.
    /// Set from the focused window's source resolution so motion feels 1:1.
    pub px_to_norm: Point2<f32>,
    /// Pixels the pointer must push past an edge before switching (avoids
    /// accidental hops). Barrier calls this "switch delay"; we use distance.
    pub edge_resistance_px: f32,
    /// Accumulated overshoot while pressing against an edge.
    overshoot: f32,
    /// Enable edge switching at all.
    pub edge_switching: bool,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            pointer: Point2::new(0.5, 0.5),
            px_to_norm: Point2::new(1.0 / 1920.0, 1.0 / 1080.0),
            edge_resistance_px: 40.0,
            overshoot: 0.0,
            edge_switching: true,
        }
    }
}

impl Router {
    /// Route a batch given the current windows (with projected quads so we
    /// know spatial adjacency) and the focused id.
    pub fn route(
        &mut self,
        windows: &[SpatialWindow],
        quads: &[ScreenQuad],
        focused: Option<WindowId>,
        events: &[InputEvent],
    ) -> Routed {
        let Some(fid) = focused else { return Routed::NoFocus };
        let Some(fw) = windows.iter().find(|w| w.id == fid) else {
            return Routed::NoFocus;
        };
        let peer = source_peer(&fw.source);

        let mut out = Vec::with_capacity(events.len());
        for ev in events {
            match *ev {
                InputEvent::MouseMove { dx, dy } => {
                    let nx = self.pointer.x + dx * self.px_to_norm.x;
                    let ny = self.pointer.y + dy * self.px_to_norm.y;
                    // Edge detection on the horizontal/vertical axis that overshoots most.
                    let edge = if nx < 0.0 {
                        Some((Edge::Left, -nx / self.px_to_norm.x))
                    } else if nx > 1.0 {
                        Some((Edge::Right, (nx - 1.0) / self.px_to_norm.x))
                    } else if ny < 0.0 {
                        Some((Edge::Top, -ny / self.px_to_norm.y))
                    } else if ny > 1.0 {
                        Some((Edge::Bottom, (ny - 1.0) / self.px_to_norm.y))
                    } else {
                        None
                    };
                    if let Some((edge, over_px)) = edge {
                        self.overshoot += over_px;
                        if self.edge_switching && self.overshoot >= self.edge_resistance_px {
                            if let Some(target) = neighbour(quads, fid, edge) {
                                self.overshoot = 0.0;
                                // Enter at the opposite edge, keep the other axis.
                                let (entered_from, p) = match edge {
                                    Edge::Left => (Edge::Right, Point2::new(0.999, ny.clamp(0.0, 1.0))),
                                    Edge::Right => (Edge::Left, Point2::new(0.001, ny.clamp(0.0, 1.0))),
                                    Edge::Top => (Edge::Bottom, Point2::new(nx.clamp(0.0, 1.0), 0.999)),
                                    Edge::Bottom => (Edge::Top, Point2::new(nx.clamp(0.0, 1.0), 0.001)),
                                };
                                self.pointer = p;
                                return Routed::Switched {
                                    to: target,
                                    entered_from,
                                };
                            }
                        }
                        self.pointer = Point2::new(nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0));
                    } else {
                        self.overshoot = 0.0;
                        self.pointer = Point2::new(nx, ny);
                    }
                    out.push(InputEvent::MouseMoveAbs {
                        x: self.pointer.x,
                        y: self.pointer.y,
                    });
                }
                InputEvent::MouseMoveAbs { x, y } => {
                    self.pointer = Point2::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
                    out.push(*ev);
                }
                other => out.push(other),
            }
        }
        Routed::Deliver {
            peer,
            window: fid,
            events: out,
        }
    }

    /// Set pointer scale from the focused window's source resolution.
    pub fn set_resolution(&mut self, w: u32, h: u32) {
        self.px_to_norm = Point2::new(1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32);
    }
}

/// Owning peer of a source.
pub fn source_peer(s: &perch_proto::Source) -> Uuid {
    use perch_proto::Source::*;
    match s {
        Display { peer, .. } | Window { peer, .. } | Virtual { peer, .. } | Dex { peer } | PhoneScreen { peer } => {
            *peer
        }
    }
}

/// Find the window adjacent to `from` in direction `edge`, using projected
/// screen quads: the nearest window whose centre lies in that direction and
/// which overlaps `from` on the perpendicular axis.
pub fn neighbour(quads: &[ScreenQuad], from: WindowId, edge: Edge) -> Option<WindowId> {
    let me = quads.iter().find(|q| q.id == from)?;
    let (mx, my) = centre(me);
    let (mw, mh) = extent(me);
    let mut best: Option<(f32, WindowId)> = None;
    for q in quads.iter().filter(|q| q.id != from && q.on_screen) {
        let (cx, cy) = centre(q);
        let (w, h) = extent(q);
        let (dir_ok, dist, overlap) = match edge {
            Edge::Left => (cx < mx, mx - cx, (my - cy).abs() < (mh + h) / 2.0),
            Edge::Right => (cx > mx, cx - mx, (my - cy).abs() < (mh + h) / 2.0),
            Edge::Top => (cy < my, my - cy, (mx - cx).abs() < (mw + w) / 2.0),
            Edge::Bottom => (cy > my, cy - my, (mx - cx).abs() < (mw + w) / 2.0),
        };
        if dir_ok && overlap && best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, q.id));
        }
    }
    best.map(|(_, id)| id)
}

fn centre(q: &ScreenQuad) -> (f32, f32) {
    let x = q.corners.iter().map(|p| p.x).sum::<f32>() / 4.0;
    let y = q.corners.iter().map(|p| p.y).sum::<f32>() / 4.0;
    (x, y)
}
fn extent(q: &ScreenQuad) -> (f32, f32) {
    let xs = q.corners.iter().map(|p| p.x);
    let ys = q.corners.iter().map(|p| p.y);
    let (minx, maxx) = xs.clone().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (miny, maxy) = ys.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    (maxx - minx, maxy - miny)
}

/// Convenience: project windows with `projector` and `pose`, then route.
pub fn route_with_projection(
    router: &mut Router,
    projector: &Projector,
    ws: &perch_space::Workspace,
    pose: &perch_proto::Pose,
    events: &[InputEvent],
) -> Routed {
    let quads = projector.project_all(ws, pose);
    router.route(&ws.windows, &quads, ws.focused, events)
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

#[cfg(all(target_os = "linux", feature = "linux-evdev"))]
pub mod linux;

#[cfg(feature = "windows")]
pub mod windows {
    //! Windows backend scaffold: `SendInput` for injection, `WH_KEYBOARD_LL` /
    //! `WH_MOUSE_LL` hooks for capture, `ClipCursor` while grabbed.
    //! See docs/PLATFORMS.md. Not implemented in v0.1.
}

#[cfg(feature = "macos")]
pub mod macos {
    //! macOS backend scaffold: `CGEventPost` for injection, `CGEventTap` for
    //! capture (needs Accessibility permission). Not implemented in v0.1.
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_proto::{Anchor, Placement, Pose, Source};
    use perch_space::{DisplayOptics, Workspace};

    fn win(peer: Uuid, yaw: f32) -> SpatialWindow {
        SpatialWindow {
            id: Uuid::new_v4(),
            source: Source::Display { peer, display_index: 0 },
            placement: Placement {
                yaw,
                pitch: 0.0,
                distance: 2.0,
                roll: 0.0,
                width_m: 1.0,
                height_m: 0.6,
            },
            anchor: Anchor::World,
            z: 0,
            opacity: 1.0,
            visible: true,
            focused: false,
        }
    }

    #[test]
    fn relative_motion_becomes_absolute() {
        let peer = Uuid::new_v4();
        let w = win(peer, 0.0);
        let mut ws = Workspace::new();
        ws.upsert(w.clone());
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let mut r = Router::default();
        let out = route_with_projection(
            &mut r,
            &pr,
            &ws,
            &Pose::identity(),
            &[InputEvent::MouseMove { dx: 192.0, dy: 0.0 }],
        );
        match out {
            Routed::Deliver {
                peer: p,
                window,
                events,
            } => {
                assert_eq!(p, peer);
                assert_eq!(window, w.id);
                assert_eq!(events, vec![InputEvent::MouseMoveAbs { x: 0.6, y: 0.5 }]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pushing_past_right_edge_switches_to_right_neighbour() {
        let laptop = Uuid::new_v4();
        let phone = Uuid::new_v4();
        let centre_w = win(laptop, 0.0);
        let right_w = win(phone, -0.5); // yaw negative = to the right
        let mut ws = Workspace::new();
        ws.upsert(centre_w.clone());
        ws.upsert(right_w.clone());
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let mut r = Router {
            edge_resistance_px: 10.0,
            ..Router::default()
        };
        // Pointer starts at centre; push 1000 px right → overshoots.
        let out = route_with_projection(
            &mut r,
            &pr,
            &ws,
            &Pose::identity(),
            &[InputEvent::MouseMove { dx: 2000.0, dy: 0.0 }],
        );
        assert_eq!(
            out,
            Routed::Switched {
                to: right_w.id,
                entered_from: Edge::Left
            }
        );
        assert!(r.pointer.x < 0.01);
    }

    #[test]
    fn no_neighbour_means_clamp_not_switch() {
        let peer = Uuid::new_v4();
        let w = win(peer, 0.0);
        let mut ws = Workspace::new();
        ws.upsert(w.clone());
        let pr = Projector::new(DisplayOptics {
            tilt: 0.0,
            ..DisplayOptics::rokid_max_1080p()
        });
        let mut r = Router::default();
        let out = route_with_projection(
            &mut r,
            &pr,
            &ws,
            &Pose::identity(),
            &[InputEvent::MouseMove { dx: 5000.0, dy: 0.0 }],
        );
        match out {
            Routed::Deliver { events, .. } => assert_eq!(events, vec![InputEvent::MouseMoveAbs { x: 1.0, y: 0.5 }]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn keys_pass_through_to_focused_peer() {
        let peer = Uuid::new_v4();
        let w = win(peer, 0.0);
        let mut ws = Workspace::new();
        ws.upsert(w);
        let mut r = Router::default();
        let ev = InputEvent::Key {
            usage: keymap::usage::KEY_A,
            down: true,
            mods: 0,
        };
        match r.route(&ws.windows, &[], ws.focused, &[ev]) {
            Routed::Deliver { peer: p, events, .. } => {
                assert_eq!(p, peer);
                assert_eq!(events, vec![ev]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(r.route(&[], &[], None, &[ev]), Routed::NoFocus);
    }
}
