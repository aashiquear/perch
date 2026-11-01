//! `perch-proto` — the shared vocabulary of the whole system.
//!
//! Every process in Perch (the Linux/macOS/Windows daemon `perchd`, the
//! Android host app, the Android/iPad touchpad controllers, the CLI) talks
//! in these types. Two encodings are supported:
//!
//! * **JSON** (`Message::to_json`) — used on the control WebSocket so phone
//!   apps and debugging tools (`websocat`) can read it.
//! * **postcard** (`Message::to_postcard`) — compact binary for the high-rate
//!   pose stream (up to 1 kHz) and input events over UDP/QUIC.
//!
//! Coordinate conventions (also in `docs/ARCHITECTURE.md`):
//!
//! * World frame is **right-handed, Y-up, -Z forward** ("RUB" like Android /
//!   `ar-drivers`): +X right, +Y up, +Z backwards (towards the wearer).
//! * Orientation is a unit quaternion rotating *head frame → world frame*.
//! * Yaw/pitch/roll in radians, intrinsic Y-X-Z (yaw about world up, then
//!   pitch about head right, then roll about head forward).
//! * Angles for window placement are **spherical**: `(yaw, pitch)` of the
//!   window centre as seen from the head origin, at `distance` metres.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use nalgebra::{UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Protocol version. Bump `MAJOR` on any incompatible wire change.
pub const PROTOCOL_VERSION: Version = Version { major: 1, minor: 2 };

/// Semantic version of the wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Incompatible change counter.
    pub major: u16,
    /// Backwards-compatible addition counter.
    pub minor: u16,
}

impl Version {
    /// Two peers can talk if the major versions match.
    pub fn compatible(&self, other: &Version) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Errors from encoding/decoding.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// JSON (de)serialisation failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// postcard (de)serialisation failed.
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    /// Peer speaks an incompatible protocol major version.
    #[error("incompatible protocol version: ours {ours}, theirs {theirs}")]
    Incompatible {
        /// Our version.
        ours: Version,
        /// Their version.
        theirs: Version,
    },
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Which kind of machine a peer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Linux desktop (perchd).
    Linux,
    /// Windows desktop (perchd).
    Windows,
    /// macOS desktop (perchd).
    MacOs,
    /// Android phone/tablet (host app, incl. Samsung DeX).
    Android,
    /// iPadOS (controller only; cannot drive glasses IMU).
    IPadOs,
    /// iOS phone (controller only).
    Ios,
    /// Anything else.
    Other,
}

/// What a peer is able to do. Advertised in [`Hello`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Peer has glasses plugged in and can produce poses + drive the display.
    pub glasses_host: bool,
    /// Peer can share its screen/windows as textures.
    pub screen_source: bool,
    /// Peer accepts injected keyboard/mouse events (is an input *sink*).
    pub input_sink: bool,
    /// Peer can send keyboard/mouse/touchpad events (is an input *source*).
    pub input_source: bool,
    /// Peer supports Samsung DeX-style desktop mode.
    pub dex: bool,
}

/// First message on every connection, both directions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// Protocol version of the sender.
    pub version: Version,
    /// Stable per-install id.
    pub peer_id: Uuid,
    /// Human-readable name ("Aashique's laptop", "S24 Ultra").
    pub name: String,
    /// Platform of the sender.
    pub platform: Platform,
    /// What the sender can do.
    pub caps: Capabilities,
}

// ---------------------------------------------------------------------------
// Tracking
// ---------------------------------------------------------------------------

/// Head pose sample (3DOF): orientation only, no translation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    /// Monotonic timestamp in microseconds, host clock.
    pub t_us: u64,
    /// Head → world rotation. On the wire: `[w, x, y, z]`.
    #[serde(with = "quat_wire")]
    pub orientation: UnitQuaternion<f32>,
    /// Angular velocity in head frame, rad/s (useful for prediction). Wire: `[x, y, z]`.
    #[serde(with = "vec3_wire")]
    pub angular_velocity: Vector3<f32>,
    /// Confidence 0..1 — drops during calibration or after a glitch.
    pub confidence: f32,
}

impl Pose {
    /// Identity pose at t = 0.
    pub fn identity() -> Self {
        Self {
            t_us: 0,
            orientation: UnitQuaternion::identity(),
            angular_velocity: Vector3::zeros(),
            confidence: 1.0,
        }
    }

    /// Yaw/pitch/roll in radians (see crate docs for the convention).
    pub fn euler(&self) -> Euler {
        let (roll, pitch, yaw) = self.orientation.euler_angles();
        // nalgebra returns (roll[x], pitch[y], yaw[z]) for an X-Y-Z intrinsic
        // decomposition. Our world is Y-up, so remap: yaw is rotation about Y.
        Euler {
            yaw: pitch,
            pitch: roll,
            roll: yaw,
        }
    }
}

/// Serialise a unit quaternion as `[w, x, y, z]` so every encoder (postcard,
/// JSON, msgpack) and every non-Rust client can read it.
pub mod quat_wire {
    use nalgebra::{Quaternion, UnitQuaternion};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    /// Serialize.
    pub fn serialize<S: Serializer>(q: &UnitQuaternion<f32>, s: S) -> Result<S::Ok, S::Error> {
        [q.w, q.i, q.j, q.k].serialize(s)
    }
    /// Deserialize.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<UnitQuaternion<f32>, D::Error> {
        let [w, x, y, z] = <[f32; 4]>::deserialize(d)?;
        Ok(UnitQuaternion::new_normalize(Quaternion::new(w, x, y, z)))
    }
}

/// Serialise a vector as `[x, y, z]`.
pub mod vec3_wire {
    use nalgebra::Vector3;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    /// Serialize.
    pub fn serialize<S: Serializer>(v: &Vector3<f32>, s: S) -> Result<S::Ok, S::Error> {
        [v.x, v.y, v.z].serialize(s)
    }
    /// Deserialize.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vector3<f32>, D::Error> {
        let a = <[f32; 3]>::deserialize(d)?;
        Ok(Vector3::from(a))
    }
}

/// Euler triple in radians.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Euler {
    /// Rotation about world up (+ = turning left).
    pub yaw: f32,
    /// Rotation about head right (+ = looking up).
    pub pitch: f32,
    /// Rotation about head forward (+ = tilting head left).
    pub roll: f32,
}

/// Calibration state reported by the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationState {
    /// No calibration has been done; drift may be large.
    Uncalibrated,
    /// Gyro bias calibration in progress; glasses must stay still.
    Calibrating {
        /// Progress 0..100.
        percent: u8,
    },
    /// Calibrated and running.
    Ready,
}

// ---------------------------------------------------------------------------
// Spatial windows
// ---------------------------------------------------------------------------

/// Stable id of a spatial window (not an OS window id; those are per-source).
pub type WindowId = Uuid;

/// Where a window sits in the wearer's world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Yaw of the window centre, radians, + = left.
    pub yaw: f32,
    /// Pitch of the window centre, radians, + = up.
    pub pitch: f32,
    /// Distance from head origin in metres (affects parallax between eyes only).
    pub distance: f32,
    /// Roll of the window about its own normal, radians. Usually 0.
    pub roll: f32,
    /// Physical width in metres at `distance`.
    pub width_m: f32,
    /// Physical height in metres at `distance`.
    pub height_m: f32,
}

/// A grid of slots on a sphere around the head that windows snap to. Slots
/// are `cols × rows` cells of `cell_w_m × cell_h_m` metres separated by
/// `gap_m`, laid out flat at `distance` and then wrapped onto the sphere
/// (each cell centre becomes a yaw/pitch). Column 0 is leftmost, row 0 is
/// the top. Since 1.2.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    /// Columns.
    pub cols: u32,
    /// Rows.
    pub rows: u32,
    /// Cell width, metres.
    pub cell_w_m: f32,
    /// Cell height, metres.
    pub cell_h_m: f32,
    /// Gap between cells, metres.
    pub gap_m: f32,
    /// Distance of the grid from the head, metres.
    pub distance: f32,
    /// Yaw of the grid centre, radians, + = left.
    pub center_yaw: f32,
    /// Pitch of the grid centre, radians, + = up.
    pub center_pitch: f32,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            cols: 3,
            rows: 1,
            cell_w_m: 1.2,
            cell_h_m: 0.8,
            gap_m: 0.1,
            distance: 2.0,
            center_yaw: 0.0,
            center_pitch: 0.0,
        }
    }
}

impl Placement {
    /// Angular width (radians) the window subtends horizontally.
    pub fn angular_width(&self) -> f32 {
        2.0 * (self.width_m / (2.0 * self.distance)).atan()
    }
    /// Angular height (radians) the window subtends vertically.
    pub fn angular_height(&self) -> f32 {
        2.0 * (self.height_m / (2.0 * self.distance)).atan()
    }
}

/// How a window follows (or doesn't follow) the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// Pinned to a world direction. The Rokid Station / spidgets behaviour.
    World,
    /// Rigidly attached to the head (plain mirror mode).
    Head,
    /// Stays put until the head turns past a threshold, then glides to the new
    /// gaze direction ("smooth follow", like Breezy / XREAL Beam).
    SmoothFollow,
}

/// Which peer and what on that peer a window shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Whole display `display_index` of peer `peer`.
    Display {
        /// Owning peer.
        peer: Uuid,
        /// Display index on that peer.
        display_index: u32,
    },
    /// A single OS window (captured individually) on peer `peer`.
    Window {
        /// Owning peer.
        peer: Uuid,
        /// Opaque per-platform window handle (X11 id, Wayland toplevel handle,
        /// HWND, CGWindowID, Android task id…).
        handle: String,
        /// Title at capture time, for UI.
        title: String,
    },
    /// A virtual display created by Perch (e.g. a 2nd/3rd monitor that only
    /// exists in the glasses).
    Virtual {
        /// Owning peer.
        peer: Uuid,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// Android DeX desktop surface.
    Dex {
        /// Owning peer.
        peer: Uuid,
    },
    /// Android phone screen (portrait mobile UI).
    PhoneScreen {
        /// Owning peer.
        peer: Uuid,
    },
}

/// A spatial window: a texture source placed somewhere in the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialWindow {
    /// Stable id.
    pub id: WindowId,
    /// What it shows.
    pub source: Source,
    /// Where it is.
    pub placement: Placement,
    /// How it follows the head.
    pub anchor: Anchor,
    /// Z-order; higher is drawn on top when windows overlap.
    pub z: i32,
    /// 0..1 opacity.
    pub opacity: f32,
    /// Hidden windows are kept in the layout but not drawn.
    pub visible: bool,
    /// Whether this window currently receives keyboard/mouse focus.
    pub focused: bool,
}

/// A named arrangement of windows the user can save/restore ("coding",
/// "gaming", "couch").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    /// Name shown in the UI.
    pub name: String,
    /// Windows in the layout.
    pub windows: Vec<SpatialWindow>,
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Middle.
    Middle,
    /// Back (thumb).
    Back,
    /// Forward (thumb).
    Forward,
}

/// Keyboard key as a USB HID usage id (page 0x07). Platform-neutral.
pub type HidUsage = u16;

/// Modifier bitmask: bit0 ctrl, bit1 shift, bit2 alt, bit3 super/cmd.
pub type Modifiers = u8;

/// An input event, platform-neutral. Phones send these from the touchpad UI;
/// `perchd` injects them into the focused window's peer via uinput / SendInput /
/// CGEvent / Android Accessibility.
///
/// Externally tagged (`{"mouse_move":{"dx":1,"dy":2}}`) so it encodes with
/// postcard on the binary channel as well as JSON.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputEvent {
    /// Relative pointer motion in logical pixels of the *target* display.
    MouseMove {
        /// dx.
        dx: f32,
        /// dy.
        dy: f32,
    },
    /// Absolute pointer position, normalised 0..1 of the target window.
    MouseMoveAbs {
        /// x.
        x: f32,
        /// y.
        y: f32,
    },
    /// Button press/release.
    MouseButton {
        /// Button.
        button: MouseButton,
        /// True on press.
        down: bool,
    },
    /// Scroll in lines (positive = up / left).
    Scroll {
        /// Horizontal.
        dx: f32,
        /// Vertical.
        dy: f32,
    },
    /// Key press/release.
    Key {
        /// HID usage.
        usage: HidUsage,
        /// True on press.
        down: bool,
        /// Modifiers held.
        mods: Modifiers,
    },
    /// Unicode text (from a phone soft keyboard). Injected as typed text.
    Text {
        /// Unicode scalar value.
        ch: char,
    },
}

// ---------------------------------------------------------------------------
// Control
// ---------------------------------------------------------------------------

/// Glasses display mode (mirrors `ar-drivers::DisplayMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// 1920x1080/1200 same image both eyes, 60 Hz.
    Mono,
    /// 3840-wide side-by-side stereo, 60 Hz.
    Stereo,
    /// 1920x1080/1200 mono at 120 Hz (recommended for low latency).
    Mono120,
    /// SBS stereo at 120 Hz.
    Stereo120,
}

/// Commands a controller (CLI, phone) can send to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Re-centre: current gaze becomes yaw = 0.
    Recenter,
    /// Start gyro-bias calibration; glasses must be still for `seconds`.
    Calibrate {
        /// Duration.
        seconds: u8,
    },
    /// Change the glasses display mode.
    SetDisplayMode {
        /// Mode.
        mode: DisplayMode,
    },
    /// Set brightness 0..100 (if the device supports it).
    SetBrightness {
        /// Percent.
        percent: u8,
    },
    /// Add or replace a window.
    UpsertWindow {
        /// Window.
        window: SpatialWindow,
    },
    /// Remove a window.
    RemoveWindow {
        /// Id.
        id: WindowId,
    },
    /// Move/resize a window.
    PlaceWindow {
        /// Id.
        id: WindowId,
        /// New placement.
        placement: Placement,
    },
    /// Pin the window where the user is currently looking.
    PinHere {
        /// Id.
        id: WindowId,
    },
    /// Change anchor behaviour.
    SetAnchor {
        /// Id.
        id: WindowId,
        /// Anchor.
        anchor: Anchor,
    },
    /// Give keyboard/mouse focus to a window (and therefore to its peer).
    Focus {
        /// Id.
        id: WindowId,
    },
    /// Cycle focus to the next visible window.
    FocusNext,
    /// Save current windows as a named layout.
    SaveLayout {
        /// Name.
        name: String,
    },
    /// Load a named layout.
    LoadLayout {
        /// Name.
        name: String,
    },
    /// Apply a built-in preset ("coding", "gaming", "couch", "single").
    ApplyPreset {
        /// Preset name.
        name: String,
    },
    /// Request a full state snapshot.
    GetState,
    /// Ask the host to let the user pick local windows/monitors to capture
    /// (on Linux this opens the desktop portal dialog). Each pick becomes a
    /// new window placed near the current gaze; `window_changed` events
    /// follow, or an `error` if the user cancelled. Since 1.1.
    PickSources {
        /// Allow selecting several sources at once.
        multiple: bool,
    },
    /// Replace the slot grid and re-place every slotted window. Since 1.2.
    SetGrid {
        /// New grid.
        grid: Grid,
    },
    /// Snap a window into grid slot (`col`, `row`). Since 1.2.
    SlotWindow {
        /// Window.
        id: WindowId,
        /// Column, 0 = leftmost.
        col: u32,
        /// Row, 0 = top.
        row: u32,
    },
    /// Put every visible window into the grid, row-major, in z order. Since 1.2.
    ArrangeGrid,
}

/// Events the host broadcasts to controllers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Glasses plugged in / detected.
    GlassesConnected {
        /// Model name.
        model: String,
        /// Serial.
        serial: String,
    },
    /// Glasses unplugged.
    GlassesDisconnected,
    /// Tracker calibration state changed.
    Calibration(CalibrationState),
    /// A low-rate pose sample for UI (the high-rate stream is binary).
    Pose(Pose),
    /// Full state snapshot.
    State(State),
    /// A window changed.
    WindowChanged(SpatialWindow),
    /// A window was removed.
    WindowRemoved {
        /// Id.
        id: WindowId,
    },
    /// Focus moved.
    FocusChanged {
        /// New focused window, if any.
        id: Option<WindowId>,
    },
    /// Human-readable error for the UI.
    Error {
        /// Message.
        message: String,
    },
}

/// Everything a controller needs to draw its UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Peers currently connected (including the host itself).
    pub peers: Vec<Hello>,
    /// Glasses model if connected.
    pub glasses: Option<String>,
    /// Current display mode.
    pub display_mode: Option<DisplayMode>,
    /// Calibration state.
    pub calibration: CalibrationState,
    /// All windows.
    pub windows: Vec<SpatialWindow>,
    /// Saved layouts (names only).
    pub layouts: Vec<String>,
    /// Focused window.
    pub focused: Option<WindowId>,
    /// The slot grid. Since 1.2 (older hosts omit it; default applies).
    #[serde(default)]
    pub grid: Grid,
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Top-level message. Everything on the wire is one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum Message {
    /// Handshake.
    Hello(Hello),
    /// Controller → host.
    Command(Command),
    /// Host → controller.
    Event(Event),
    /// Input from a controller, to be injected at the focused window's peer.
    Input {
        /// Events in this batch (batched every ~8 ms to save packets).
        events: Vec<InputEvent>,
    },
    /// High-rate pose (host → compositor peers).
    Pose(Pose),
    /// Liveness.
    Ping {
        /// Echoed back.
        nonce: u32,
    },
    /// Liveness reply.
    Pong {
        /// Echo.
        nonce: u32,
    },
}

impl Message {
    /// Encode as JSON (control channel).
    pub fn to_json(&self) -> Result<String, ProtoError> {
        Ok(serde_json::to_string(self)?)
    }
    /// Decode from JSON.
    pub fn from_json(s: &str) -> Result<Self, ProtoError> {
        Ok(serde_json::from_str(s)?)
    }
}

/// High-rate binary packet (UDP/QUIC datagram). Externally tagged so it
/// encodes with postcard; `Message` is internally tagged for JSON readability
/// and is **not** valid on the binary channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamPacket {
    /// Head pose at up to IMU rate (host → renderers).
    Pose(Pose),
    /// Batched input events (controller → host).
    Input(Vec<InputEvent>),
    /// Liveness.
    Ping(u32),
    /// Liveness reply.
    Pong(u32),
}

impl StreamPacket {
    /// Encode as postcard.
    pub fn to_postcard(&self) -> Result<Vec<u8>, ProtoError> {
        Ok(postcard::to_stdvec(self)?)
    }
    /// Decode from postcard.
    pub fn from_postcard(b: &[u8]) -> Result<Self, ProtoError> {
        Ok(postcard::from_bytes(b)?)
    }
}

/// Default port for the control WebSocket.
pub const DEFAULT_CONTROL_PORT: u16 = 7420;
/// Default port for the binary UDP pose/input stream.
pub const DEFAULT_STREAM_PORT: u16 = 7421;
/// mDNS service type perchd advertises.
pub const MDNS_SERVICE: &str = "_perch._tcp.local.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_command() {
        let id = Uuid::new_v4();
        let m = Message::Command(Command::PlaceWindow {
            id,
            placement: Placement {
                yaw: 0.3,
                pitch: -0.1,
                distance: 2.0,
                roll: 0.0,
                width_m: 1.6,
                height_m: 0.9,
            },
        });
        let s = m.to_json().unwrap();
        assert!(s.contains("\"cmd\":\"place_window\""));
        assert_eq!(Message::from_json(&s).unwrap(), m);

        // Since 1.2.
        let m = Message::Command(Command::SlotWindow { id, col: 2, row: 0 });
        let s = m.to_json().unwrap();
        assert!(s.contains("\"cmd\":\"slot_window\""));
        assert_eq!(Message::from_json(&s).unwrap(), m);
        let g: Grid = serde_json::from_str("{\"cols\":4}").unwrap();
        assert_eq!(g.cols, 4);
        assert_eq!(g.rows, Grid::default().rows);

        // Since 1.1.
        let m = Message::Command(Command::PickSources { multiple: true });
        let s = m.to_json().unwrap();
        assert_eq!(s, r#"{"msg":"command","cmd":"pick_sources","multiple":true}"#);
        assert_eq!(Message::from_json(&s).unwrap(), m);
    }

    #[test]
    fn postcard_roundtrip_pose() {
        let p = Pose {
            t_us: 123_456,
            orientation: UnitQuaternion::from_euler_angles(0.1, 0.2, 0.3),
            angular_velocity: Vector3::new(1.0, 2.0, 3.0),
            confidence: 0.9,
        };
        let m = StreamPacket::Pose(p);
        let b = m.to_postcard().unwrap();
        assert!(b.len() < 64, "pose packet should be tiny, got {}", b.len());
        assert_eq!(StreamPacket::from_postcard(&b).unwrap(), m);
        // Pose also round-trips through the JSON control channel.
        let j = Message::Pose(p).to_json().unwrap();
        assert!(j.contains("\"orientation\":["));
        assert_eq!(Message::from_json(&j).unwrap(), Message::Pose(p));
    }

    #[test]
    fn input_batch_postcard() {
        let m = StreamPacket::Input(vec![
            InputEvent::MouseMove { dx: 3.0, dy: -1.5 },
            InputEvent::Key {
                usage: 0x04,
                down: true,
                mods: 0b0001,
            },
            InputEvent::Text { ch: 'é' },
        ]);
        let b = m.to_postcard().unwrap();
        assert_eq!(StreamPacket::from_postcard(&b).unwrap(), m);
        let j = serde_json::to_string(&InputEvent::MouseMove { dx: 1.0, dy: 2.0 }).unwrap();
        assert_eq!(j, r#"{"mouse_move":{"dx":1.0,"dy":2.0}}"#);
    }

    #[test]
    fn angular_size() {
        let p = Placement {
            yaw: 0.0,
            pitch: 0.0,
            distance: 2.0,
            roll: 0.0,
            width_m: 2.0,
            height_m: 1.0,
        };
        // width 2 m at 2 m → 2*atan(0.5) ≈ 53.13°
        assert!((p.angular_width().to_degrees() - 53.13).abs() < 0.05);
    }

    #[test]
    fn version_compat() {
        assert!(PROTOCOL_VERSION.compatible(&Version { major: 1, minor: 7 }));
        assert!(!PROTOCOL_VERSION.compatible(&Version { major: 2, minor: 0 }));
    }
}

#[cfg(test)]
mod wire_format_tests {
    //! These pin the exact bytes so the Kotlin (`Postcard.kt`) and Swift
    //! (`Postcard.swift`) encoders can be checked against them.
    use super::*;

    #[test]
    fn ping_bytes() {
        assert_eq!(StreamPacket::Ping(1).to_postcard().unwrap(), vec![2, 1]);
        assert_eq!(StreamPacket::Ping(300).to_postcard().unwrap(), vec![2, 0xAC, 0x02]);
    }

    #[test]
    fn input_bytes() {
        let b = StreamPacket::Input(vec![
            InputEvent::MouseMove { dx: 1.0, dy: -2.0 },
            InputEvent::MouseButton {
                button: MouseButton::Right,
                down: true,
            },
            InputEvent::Scroll { dx: 0.0, dy: 3.0 },
            InputEvent::Key {
                usage: 0x04,
                down: false,
                mods: 0b0011,
            },
            InputEvent::Text { ch: 'é' },
        ])
        .to_postcard()
        .unwrap();
        let expected: Vec<u8> = vec![
            1, // StreamPacket::Input
            5, // 5 events
            0, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0xC0, // MouseMove 1.0, -2.0 (f32 LE)
            2, 1, 1, // MouseButton: variant 2, button Right(=1), down=true
            3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x40, // Scroll 0.0, 3.0
            4, 0x04, 0, 0x03, // Key: usage varint 4, down=false, mods=3
            5, 2, 0xC3, 0xA9, // Text: char as UTF-8 string, len 2
        ];
        assert_eq!(b, expected);
    }
}
