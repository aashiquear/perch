//! The engine owns the workspace and applies commands. It is single-threaded
//! (driven by the control task) and pure with respect to I/O except for
//! persistence, which makes it easy to test.

use crate::config::Paths;
use perch_input::{Routed, Router};
use perch_proto::{
    Anchor, CalibrationState, Command, DisplayMode, Event, Hello, InputEvent, Pose, SpatialWindow, State, WindowId,
};
use perch_space::{DisplayOptics, Projector, Workspace};
use std::collections::HashMap;
use uuid::Uuid;

/// Things the engine asks the outside world to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Broadcast an event to all controllers.
    Broadcast(Event),
    /// Deliver input to a peer (local sink if `peer == self`).
    DeliverInput {
        /// Destination.
        peer: Uuid,
        /// Events.
        events: Vec<InputEvent>,
    },
    /// Ask the tracker to recenter.
    Recenter,
    /// Ask the tracker to calibrate.
    Calibrate {
        /// Seconds.
        seconds: u8,
    },
    /// Ask the HAL to change display mode.
    SetDisplayMode(DisplayMode),
    /// Ask the HAL to set brightness.
    SetBrightness(u8),
    /// Open the platform's source picker (portal dialog) and add what the
    /// user chose via [`Engine::add_picked_windows`].
    PickSources {
        /// Allow several sources at once.
        multiple: bool,
    },
}

/// Engine state.
pub struct Engine {
    /// Our own hello.
    pub me: Hello,
    /// Spatial windows.
    pub ws: Workspace,
    /// Input router.
    pub router: Router,
    /// Projector (optics may change with display mode).
    pub projector: Projector,
    /// Last pose from the tracker.
    pub pose: Pose,
    /// Glasses model if connected.
    pub glasses: Option<String>,
    /// Current display mode.
    pub display_mode: Option<DisplayMode>,
    /// Calibration state.
    pub calibration: CalibrationState,
    /// Known peers by id.
    pub peers: HashMap<Uuid, Hello>,
    /// Gaze yaw when the last `pick_sources` arrived — the user then looks
    /// down at the laptop to use the dialog, so the pose at completion time
    /// is the wrong place for the new windows.
    pick_yaw: Option<f32>,
    paths: Option<Paths>,
}

impl Engine {
    /// Create, loading the persisted workspace if present.
    pub fn new(me: Hello, optics: DisplayOptics, paths: Option<Paths>) -> Self {
        let ws = paths
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p.workspace_file()).ok())
            .and_then(|s| Workspace::from_json(&s).ok())
            .unwrap_or_default();
        let mut peers = HashMap::new();
        peers.insert(me.peer_id, me.clone());
        Self {
            me,
            ws,
            router: Router::default(),
            projector: Projector::new(optics),
            pose: Pose::identity(),
            glasses: None,
            display_mode: None,
            calibration: CalibrationState::Uncalibrated,
            peers,
            pick_yaw: None,
            paths,
        }
    }

    /// Persist the workspace (best effort).
    pub fn save(&self) {
        if let Some(p) = &self.paths {
            let _ = std::fs::create_dir_all(&p.dir);
            if let Err(e) = std::fs::write(p.workspace_file(), self.ws.to_json()) {
                tracing::warn!("could not save workspace: {e}");
            }
        }
    }

    /// Snapshot for controllers.
    pub fn state(&self) -> State {
        State {
            peers: self.peers.values().cloned().collect(),
            glasses: self.glasses.clone(),
            display_mode: self.display_mode,
            calibration: self.calibration,
            windows: self.ws.windows.clone(),
            layouts: self.ws.layouts.keys().cloned().collect(),
            focused: self.ws.focused,
            grid: self.ws.grid,
        }
    }

    /// A peer connected.
    pub fn peer_joined(&mut self, h: Hello) -> Vec<Effect> {
        self.peers.insert(h.peer_id, h);
        vec![Effect::Broadcast(Event::State(self.state()))]
    }

    /// A peer left: hide its windows (keep them in the layout for when it returns).
    pub fn peer_left(&mut self, id: Uuid) -> Vec<Effect> {
        self.peers.remove(&id);
        let mut fx = Vec::new();
        for w in &mut self.ws.windows {
            if perch_input::source_peer(&w.source) == id && w.visible {
                w.visible = false;
                fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
            }
        }
        if let Some(f) = self.ws.focused {
            if self.ws.get(f).is_none_or(|w| !w.visible) {
                let next = self.ws.focus_next();
                fx.push(Effect::Broadcast(Event::FocusChanged { id: next }));
            }
        }
        fx
    }

    /// Apply a command from `from`.
    pub fn command(&mut self, _from: Uuid, cmd: Command) -> Vec<Effect> {
        let mut fx = Vec::new();
        let mut changed: Option<WindowId> = None;
        match cmd {
            Command::Recenter => fx.push(Effect::Recenter),
            Command::Calibrate { seconds } => fx.push(Effect::Calibrate { seconds }),
            Command::SetDisplayMode { mode } => fx.push(Effect::SetDisplayMode(mode)),
            Command::SetBrightness { percent } => fx.push(Effect::SetBrightness(percent.min(100))),
            Command::UpsertWindow { window } => {
                changed = Some(window.id);
                self.ws.upsert(window);
            }
            Command::RemoveWindow { id } => {
                if self.ws.remove(id).is_ok() {
                    fx.push(Effect::Broadcast(Event::WindowRemoved { id }));
                    fx.push(Effect::Broadcast(Event::FocusChanged { id: self.ws.focused }));
                } else {
                    fx.push(err("no such window"));
                }
            }
            Command::PlaceWindow { id, placement } => match self.ws.place(id, placement) {
                Ok(()) => changed = Some(id),
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::PinHere { id } => match self.ws.pin_here(id, &self.pose) {
                Ok(()) => changed = Some(id),
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::SetAnchor { id, anchor } => match self.ws.set_anchor(id, anchor) {
                Ok(()) => changed = Some(id),
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::Focus { id } => match self.ws.focus(id) {
                Ok(()) => {
                    self.on_focus_change();
                    fx.push(Effect::Broadcast(Event::FocusChanged { id: Some(id) }));
                }
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::FocusNext => {
                let id = self.ws.focus_next();
                self.on_focus_change();
                fx.push(Effect::Broadcast(Event::FocusChanged { id }));
            }
            Command::SaveLayout { name } => {
                self.ws.save_layout(&name);
                fx.push(Effect::Broadcast(Event::State(self.state())));
            }
            Command::LoadLayout { name } => match self.ws.load_layout(&name) {
                Ok(()) => fx.push(Effect::Broadcast(Event::State(self.state()))),
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::ApplyPreset { name } => match self.ws.apply_preset(&name, self.me.peer_id) {
                Ok(()) => fx.push(Effect::Broadcast(Event::State(self.state()))),
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::GetState => fx.push(Effect::Broadcast(Event::State(self.state()))),
            Command::PickSources { multiple } => {
                self.pick_yaw = Some(perch_space::gaze_angles(&self.pose).0);
                fx.push(Effect::PickSources { multiple });
            }
            Command::SetGrid { grid } => {
                let moved = self.ws.set_grid(grid);
                for id in moved {
                    if let Some(w) = self.ws.get(id) {
                        fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
                    }
                }
                fx.push(Effect::Broadcast(Event::State(self.state())));
            }
            Command::SlotWindow { id, row, index } => match self.ws.set_slot(id, row, index) {
                Ok(moved) => {
                    for m in moved {
                        if let Some(w) = self.ws.get(m) {
                            fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
                        }
                    }
                    // Always announce the slotted window even if it did not move.
                    changed = Some(id);
                }
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::SetWindowSpan { id, span } => match self.ws.set_span(id, span) {
                Ok(moved) => {
                    for m in moved {
                        if let Some(w) = self.ws.get(m) {
                            fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
                        }
                    }
                }
                Err(e) => fx.push(err(&e.to_string())),
            },
            Command::SetWindowPixels { id, width, height } => {
                if self.ws.get(id).is_none() {
                    fx.push(err("no such window"));
                } else {
                    for m in self.ws.set_size_px(id, (width, height)) {
                        if let Some(w) = self.ws.get(m) {
                            fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
                        }
                    }
                    changed = Some(id);
                }
            }
            Command::ArrangeGrid => {
                let moved = self.ws.arrange_grid();
                for id in moved {
                    if let Some(w) = self.ws.get(id) {
                        fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
                    }
                }
                fx.push(Effect::Broadcast(Event::State(self.state())));
            }
        }
        if let Some(id) = changed {
            if let Some(w) = self.ws.get(id) {
                fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
            }
        }
        if !fx.is_empty() {
            self.save();
        }
        fx
    }

    fn on_focus_change(&mut self) {
        if let Some(w) = self.ws.focused.and_then(|f| self.ws.get(f)) {
            let (rw, rh) = source_resolution(w);
            self.router.set_resolution(rw, rh);
        }
    }

    /// Route a batch of input from a controller or local devices.
    pub fn input(&mut self, events: Vec<InputEvent>) -> Vec<Effect> {
        let mut fx = Vec::new();
        let mut pending = events;
        // At most one edge switch per batch to keep it simple.
        for _ in 0..2 {
            let quads = self.projector.project_all(&self.ws, &self.pose);
            match self.router.route(&self.ws.windows, &quads, self.ws.focused, &pending) {
                Routed::Deliver { peer, events, .. } => {
                    fx.push(Effect::DeliverInput { peer, events });
                    break;
                }
                Routed::Switched { to, .. } => {
                    let _ = self.ws.focus(to);
                    self.on_focus_change();
                    fx.push(Effect::Broadcast(Event::FocusChanged { id: Some(to) }));
                    // Re-deliver only the non-motion events; motion was consumed by the hop.
                    pending.retain(|e| !matches!(e, InputEvent::MouseMove { .. }));
                    if pending.is_empty() {
                        break;
                    }
                }
                Routed::NoFocus => break,
            }
        }
        fx
    }

    /// Glasses temple button.
    pub fn button(&mut self, action: &str) -> Vec<Effect> {
        match action {
            "recenter" => vec![Effect::Recenter],
            "focus_next" => self.command(self.me.peer_id, Command::FocusNext),
            _ => vec![],
        }
    }

    /// New pose from the tracker.
    pub fn set_pose(&mut self, p: Pose) {
        self.pose = p;
    }

    /// Where files live (None in tests).
    pub fn paths(&self) -> Option<&Paths> {
        self.paths.as_ref()
    }

    /// Add windows for freshly captured sources. Each is appended to the
    /// emptiest grid row, sized from its pixel size. Sources that already
    /// have a window are skipped (restore after restart), so this is
    /// idempotent. When real windows arrive, our own `Virtual` placeholders
    /// (preset demos) go away first.
    /// Drop windows whose capture handles belong to a previous run. Portal
    /// stream indices restart at zero every launch, so a saved `portal:1`
    /// would silently show whatever stream 1 is now (a duplicate of another
    /// window, or nothing). The restore re-adds the current selection.
    pub fn prune_local_captures(&mut self) -> Vec<Effect> {
        let me = self.me.peer_id;
        let stale: Vec<WindowId> = self
            .ws
            .windows
            .iter()
            .filter(|w| match &w.source {
                perch_proto::Source::Window { peer, handle, .. } => *peer == me && handle.starts_with("portal:"),
                perch_proto::Source::Display { peer, .. } => *peer == me,
                _ => false,
            })
            .map(|w| w.id)
            .collect();
        let mut fx = Vec::new();
        for id in stale {
            if self.ws.remove(id).is_ok() {
                fx.push(Effect::Broadcast(Event::WindowRemoved { id }));
            }
        }
        fx
    }

    pub fn add_picked_windows(&mut self, picked: Vec<(perch_proto::Source, Option<(u32, u32)>)>) -> Vec<Effect> {
        self.pick_yaw = None;
        let mut fx = Vec::new();
        let is_new = |ws: &Workspace, s: &perch_proto::Source| !ws.windows.iter().any(|w| w.source == *s);
        if picked.iter().any(|(s, _)| is_new(&self.ws, s)) {
            let me = self.me.peer_id;
            let placeholders: Vec<WindowId> = self
                .ws
                .windows
                .iter()
                .filter(|w| matches!(w.source, perch_proto::Source::Virtual { peer, .. } if peer == me))
                .map(|w| w.id)
                .collect();
            for id in placeholders {
                if self.ws.remove(id).is_ok() {
                    fx.push(Effect::Broadcast(Event::WindowRemoved { id }));
                }
            }
        }
        let mut touched: Vec<WindowId> = Vec::new();
        for (source, size) in picked {
            if !is_new(&self.ws, &source) {
                continue;
            }
            let aspect = match size {
                Some((w, h)) if w > 0 && h > 0 => w as f32 / h as f32,
                _ => 16.0 / 9.0,
            };
            let id = Uuid::new_v4();
            self.ws.upsert(SpatialWindow {
                id,
                source,
                placement: perch_proto::Placement {
                    yaw: 0.0,
                    pitch: 0.0,
                    distance: self.ws.grid.distance,
                    roll: 0.0,
                    width_m: self.ws.grid.row_h_m * aspect,
                    height_m: self.ws.grid.row_h_m,
                },
                anchor: Anchor::World,
                z: self.ws.windows.len() as i32,
                opacity: 1.0,
                visible: true,
                focused: false,
            });
            if let Some(px) = size {
                self.ws.sizes.insert(id, px);
            }
            let row = self.ws.emptiest_row();
            touched.push(id);
            if let Ok(moved) = self.ws.set_slot(id, row, u32::MAX) {
                touched.extend(moved);
            }
        }
        touched.sort();
        touched.dedup();
        for id in touched {
            if let Some(w) = self.ws.get(id) {
                fx.push(Effect::Broadcast(Event::WindowChanged(w.clone())));
            }
        }
        if !fx.is_empty() {
            self.on_focus_change();
            fx.push(Effect::Broadcast(Event::FocusChanged { id: self.ws.focused }));
            self.save();
        }
        fx
    }

    /// Per-frame placements (advances smooth-follow).
    #[allow(dead_code)]
    pub fn tick(&mut self, dt: f32) -> Vec<(WindowId, perch_proto::Placement)> {
        self.ws.tick(&self.pose, dt)
    }

    /// Visible windows with their current (possibly smooth-followed) placement
    /// substituted, ready for projection.
    #[allow(dead_code)]
    pub fn windows_for_render(&mut self, dt: f32) -> Vec<SpatialWindow> {
        let placements = self.tick(dt);
        self.ws
            .windows
            .iter()
            .filter(|w| w.visible)
            .map(|w| {
                let mut w = w.clone();
                if let Some((_, p)) = placements.iter().find(|(id, _)| *id == w.id) {
                    w.placement = *p;
                    if w.anchor == Anchor::SmoothFollow {
                        w.anchor = Anchor::World;
                    }
                }
                w
            })
            .collect()
    }
}

fn err(m: &str) -> Effect {
    Effect::Broadcast(Event::Error { message: m.to_string() })
}

/// Best-known pixel resolution of a window's source (for pointer scaling).
pub fn source_resolution(w: &SpatialWindow) -> (u32, u32) {
    match &w.source {
        perch_proto::Source::Virtual { width, height, .. } => (*width, *height),
        perch_proto::Source::PhoneScreen { .. } => (1080, 2340),
        _ => (1920, 1080),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_proto::{Capabilities, Placement, Platform, Source, PROTOCOL_VERSION};

    fn me() -> Hello {
        Hello {
            version: PROTOCOL_VERSION,
            peer_id: Uuid::new_v4(),
            name: "test".into(),
            platform: Platform::Linux,
            caps: Capabilities::default(),
        }
    }

    #[test]
    fn preset_then_pin_then_state() {
        let mut e = Engine::new(me(), DisplayOptics::rokid_max_1080p(), None);
        let fx = e.command(e.me.peer_id, Command::ApplyPreset { name: "coding".into() });
        assert!(matches!(fx[0], Effect::Broadcast(Event::State(_))));
        assert_eq!(e.ws.windows.len(), 3);
        let id = e.ws.windows[1].id;
        e.set_pose(Pose {
            orientation: nalgebra::UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), 0.9),
            ..Pose::identity()
        });
        let fx = e.command(e.me.peer_id, Command::PinHere { id });
        match &fx[0] {
            Effect::Broadcast(Event::WindowChanged(w)) => assert!((w.placement.yaw - 0.9).abs() < 1e-4),
            other => panic!("{other:?}"),
        }
        let s = e.state();
        assert_eq!(s.windows.len(), 3);
        assert_eq!(s.focused, Some(e.ws.windows[0].id));
    }

    #[test]
    fn input_goes_to_focused_peer_and_switches() {
        let mut e = Engine::new(
            me(),
            DisplayOptics {
                tilt: 0.0,
                ..DisplayOptics::rokid_max_1080p()
            },
            None,
        );
        let laptop = e.me.peer_id;
        let phone = Uuid::new_v4();
        let a = SpatialWindow {
            id: Uuid::new_v4(),
            source: Source::Display {
                peer: laptop,
                display_index: 0,
            },
            placement: Placement {
                yaw: 0.0,
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
        };
        let b = SpatialWindow {
            id: Uuid::new_v4(),
            source: Source::PhoneScreen { peer: phone },
            placement: Placement {
                yaw: -0.5,
                ..a.placement
            },
            ..a.clone()
        };
        e.command(laptop, Command::UpsertWindow { window: a.clone() });
        e.command(laptop, Command::UpsertWindow { window: b.clone() });
        let fx = e.input(vec![InputEvent::MouseMove { dx: 10.0, dy: 0.0 }]);
        assert!(matches!(&fx[0], Effect::DeliverInput { peer, .. } if *peer == laptop));
        e.router.edge_resistance_px = 1.0;
        let fx = e.input(vec![
            InputEvent::MouseMove { dx: 5000.0, dy: 0.0 },
            InputEvent::Text { ch: 'x' },
        ]);
        assert_eq!(fx[0], Effect::Broadcast(Event::FocusChanged { id: Some(b.id) }));
        assert!(
            matches!(&fx[1], Effect::DeliverInput { peer, events } if *peer == phone && events == &vec![InputEvent::Text { ch: 'x' }])
        );
    }

    #[test]
    fn prune_drops_only_this_runs_portal_captures() {
        let mut e = Engine::new(me(), DisplayOptics::rokid_max_1080p(), None);
        let mine = e.me.peer_id;
        let other = Uuid::new_v4();
        let win = |peer, handle: &str| Source::Window {
            peer,
            handle: handle.to_string(),
            title: String::new(),
        };
        e.add_picked_windows(vec![
            (win(mine, "portal:0"), None),
            (win(other, "portal:0"), None),
            (win(mine, "x11:42"), None),
            (
                Source::Display {
                    peer: mine,
                    display_index: 0,
                },
                None,
            ),
        ]);
        assert_eq!(e.ws.windows.len(), 4);
        let fx = e.prune_local_captures();
        assert_eq!(fx.len(), 2, "local portal window + local display");
        let left: Vec<&Source> = e.ws.windows.iter().map(|w| &w.source).collect();
        assert!(left.contains(&&win(other, "portal:0")), "remote peer's window stays");
        assert!(left.contains(&&win(mine, "x11:42")), "non-portal handle stays");
    }

    #[test]
    fn picked_windows_fan_out_right_of_gaze_and_do_not_duplicate() {
        let mut e = Engine::new(me(), DisplayOptics::rokid_max_1080p(), None);
        let mine = e.me.peer_id;
        let src = |i: u32| Source::Window {
            peer: mine,
            handle: format!("portal:{i}"),
            title: String::new(),
        };
        e.ws.apply_preset("coding", mine).unwrap();
        assert_eq!(e.ws.windows.len(), 3, "preset placeholders");
        let fx = e.add_picked_windows(vec![(src(0), Some((1920, 1080))), (src(1), Some((1080, 1920)))]);
        assert_eq!(e.ws.windows.len(), 2, "placeholders replaced by picked windows");
        assert_eq!(
            fx.iter()
                .filter(|f| matches!(f, Effect::Broadcast(Event::WindowRemoved { .. })))
                .count(),
            3
        );
        assert!(e.ws.windows.iter().all(|w| w.placement.pitch == 0.0), "eye level");
        let (a, b) = (&e.ws.windows[0], &e.ws.windows[1]);
        assert_eq!(e.ws.slots[&a.id], (0, 0), "first picked window starts row 0");
        assert_eq!(e.ws.slots[&b.id], (0, 1), "second one is appended to the row");
        assert!(
            b.placement.yaw < a.placement.yaw,
            "second window to the right (yaw + = left)"
        );
        assert!(
            a.placement.width_m > b.placement.width_m,
            "1920 px wide beats 1080 px wide"
        );
        assert!((a.placement.width_m / a.placement.height_m - 16.0 / 9.0).abs() < 1e-3);
        assert!((b.placement.width_m / b.placement.height_m - 9.0 / 16.0).abs() < 1e-3);
        assert!(fx
            .iter()
            .any(|f| matches!(f, Effect::Broadcast(Event::FocusChanged { id: Some(_) }))));
        // Restoring the same sources after a restart adds nothing.
        let fx = e.add_picked_windows(vec![(src(0), None), (src(1), None)]);
        assert!(fx.is_empty());
        assert_eq!(e.ws.windows.len(), 2);
    }

    #[test]
    fn peer_leaving_hides_its_windows() {
        let mut e = Engine::new(me(), DisplayOptics::rokid_max_1080p(), None);
        let phone = Uuid::new_v4();
        let w = SpatialWindow {
            id: Uuid::new_v4(),
            source: Source::Dex { peer: phone },
            placement: Placement {
                yaw: 0.0,
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
        };
        e.command(phone, Command::UpsertWindow { window: w.clone() });
        let fx = e.peer_left(phone);
        assert!(matches!(&fx[0], Effect::Broadcast(Event::WindowChanged(x)) if !x.visible));
    }
}
