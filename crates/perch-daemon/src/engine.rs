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
