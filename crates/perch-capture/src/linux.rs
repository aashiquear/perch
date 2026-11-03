//! Linux capture: xdg-desktop-portal ScreenCast → PipeWire (feature
//! `linux-pipewire`). Works on GNOME, KDE, wlroots and X11 via the portal.
//!
//! Flow:
//!
//! 1. [`PortalBackend::pick`] runs the portal dialog on a dedicated thread
//!    (`Screencast::create_session` → `select_sources` → `start` →
//!    `open_pipe_wire_remote`). The user picks one or more windows/monitors.
//! 2. Each picked stream becomes a [`Source`] whose handle is `portal:<n>`
//!    (`n` = index within this backend). The portal gives no window titles.
//! 3. Each portal session gets exactly one PipeWire connection ([`PwSession`],
//!    its own thread + main loop) — the portal hands out one socket per
//!    session and two cores on a dup of the same socket corrupt each other.
//!    [`PortalBackend::open`] adds a stream to that loop; it negotiates a
//!    CPU-visible BGRx/BGRA/RGBx/RGBA format and publishes the latest frame
//!    (older frames are dropped) as [`Frame::Rgba`].
//! 4. The restore token from the last pick is exposed via
//!    [`PortalBackend::restore_token`]; passing it to [`PortalBackend::restore`]
//!    on the next start re-creates the same streams with no dialog.
//!
//! Zero-copy DMA-BUF import is a later step (ROADMAP M2); 1–3 windows at
//! 1080p/60 through CPU memory cost ~0.5 GB/s per window, which is fine.
//!
//! Virtual monitors (GNOME `RecordVirtual`, `wlr-virtual-output`) are not
//! implemented yet.

use super::{now_us, CaptureBackend, CaptureError, CaptureSource, Frame, PixelFormat};
use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use perch_proto::Source;
use perch_proto::{InputEvent, MouseButton};
use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// One stream the portal handed us.
#[derive(Debug, Clone)]
pub struct PickedStream {
    /// Index within this backend (`portal:<index>` handle).
    pub index: u32,
    /// PipeWire node id to connect to.
    pub node_id: u32,
    /// Size reported by the portal, if any.
    pub size: Option<(u32, u32)>,
    /// Monitor or window.
    pub kind: Option<SourceType>,
    /// Portal's opaque per-source id, if it gives one; picking the same
    /// window twice then maps to the same [`Source`] instead of a duplicate.
    pub portal_id: Option<String>,
    /// Index of the portal session (for input injection).
    pub session_index: usize,
    /// Window streams are monitor-sized with the window drawn inside; this
    /// is the detected window rectangle in stream pixels, once known.
    pub crop: Arc<Mutex<Option<crate::Crop>>>,
    /// Whether that session is a remote-desktop session (can inject).
    pub remote: bool,
    /// Logical desktop position (monitors only).
    pub position: Option<(i32, i32)>,
    /// The PipeWire connection of the portal session that owns this node.
    pub(crate) session: Arc<PwSession>,
}

enum PortalReq {
    Pick {
        multiple: bool,
        restore_token: Option<String>,
        reply: mpsc::Sender<Result<PickResult, CaptureError>>,
    },
    /// Close every live portal session (persists the restore selection).
    Close { reply: mpsc::Sender<()> },
    /// Inject input through a remote-desktop session.
    Inject {
        session: usize,
        node_id: u32,
        /// Stream rectangle that the window's normalised coordinates span.
        rect: crate::Crop,
        /// Divisor from stream pixels to the units Mutter expects (window
        /// streams: logical surface pixels; monitor streams: stream pixels).
        scale: f32,
        events: Vec<InputEvent>,
    },
}

/// One picked stream as the portal describes it.
#[derive(Debug, Clone)]
struct RawStream {
    node_id: u32,
    size: Option<(u32, u32)>,
    kind: Option<SourceType>,
    portal_id: Option<String>,
    position: Option<(i32, i32)>,
}

/// One logical monitor as GNOME lays it out (logical pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct LogicalMonitor {
    /// Connector, e.g. "DP-5".
    pub connector: String,
    /// Position.
    pub x: i32,
    /// Position.
    pub y: i32,
    /// Logical size (mode size ÷ scale).
    pub w: f32,
    /// Logical size.
    pub h: f32,
    /// Scale factor.
    pub scale: f32,
}

struct PickResult {
    streams: Vec<RawStream>,
    layout: Vec<LogicalMonitor>,
    restore_token: Option<String>,
    fd: OwnedFd,
    /// Filled in by the portal thread once the session is stored.
    session_index: usize,
    remote: bool,
}

/// Keeps a portal session alive for as long as its streams are used.
enum LiveSession {
    /// ScreenCast only (compositor without a RemoteDesktop portal).
    Cast {
        _proxy: Screencast<'static>,
        session: Session<'static, Screencast<'static>>,
    },
    /// RemoteDesktop session with screen-cast sources: can inject input.
    Remote {
        _cast: Screencast<'static>,
        proxy: RemoteDesktop<'static>,
        session: Session<'static, RemoteDesktop<'static>>,
    },
}

impl LiveSession {
    async fn close(&self) -> Result<(), ashpd::Error> {
        match self {
            LiveSession::Cast { session, .. } => session.close().await,
            LiveSession::Remote { session, .. } => session.close().await,
        }
    }
}

/// Portal-backed capture.
pub struct PortalBackend {
    peer: Uuid,
    req_tx: mpsc::Sender<PortalReq>,
    streams: Vec<PickedStream>,
    restore_token: Option<String>,
    layout: Vec<LogicalMonitor>,
}

impl PortalBackend {
    /// Connect to the portal. `peer` is our own peer id, used in the
    /// [`Source`]s this backend produces.
    pub fn new(peer: Uuid) -> Result<Self, CaptureError> {
        let (req_tx, req_rx) = mpsc::channel::<PortalReq>();
        std::thread::Builder::new()
            .name("perch-portal".into())
            .spawn(move || portal_thread(req_rx))
            .map_err(|e| CaptureError::Other(format!("spawn portal thread: {e}")))?;
        Ok(Self {
            peer,
            req_tx,
            streams: Vec::new(),
            restore_token: None,
            layout: Vec::new(),
        })
    }

    fn request(&mut self, multiple: bool, restore_token: Option<String>) -> Result<Vec<Source>, CaptureError> {
        let (reply, rx) = mpsc::channel();
        self.req_tx
            .send(PortalReq::Pick {
                multiple,
                restore_token,
                reply,
            })
            .map_err(|_| CaptureError::Other("portal thread gone".into()))?;
        // The user may take a while in the dialog.
        let res = rx
            .recv_timeout(Duration::from_secs(300))
            .map_err(|_| CaptureError::Other("portal timed out".into()))??;
        let session = Arc::new(PwSession::start(res.fd)?);
        if !res.layout.is_empty() {
            self.layout = res.layout;
        }
        let mut added = Vec::new();
        for RawStream {
            node_id,
            size,
            kind,
            portal_id,
            position,
        } in res.streams
        {
            if let Some(existing) = portal_id
                .as_deref()
                .and_then(|pid| self.streams.iter().find(|s| s.portal_id.as_deref() == Some(pid)))
            {
                // Same window picked again: hand back the existing source
                // (the engine ignores sources it already shows).
                tracing::info!(index = existing.index, "source already picked; not duplicating");
                added.push(self.source_for(existing.index));
                continue;
            }
            let index = self.streams.len() as u32;
            self.streams.push(PickedStream {
                index,
                node_id,
                size,
                kind,
                portal_id,
                session_index: res.session_index,
                crop: Default::default(),
                remote: res.remote,
                position,
                session: Arc::clone(&session),
            });
            added.push(self.source_for(index));
        }
        if res.restore_token.is_some() {
            self.restore_token = res.restore_token;
        }
        tracing::info!(streams = added.len(), "portal capture started");
        Ok(added)
    }

    fn source_for(&self, index: u32) -> Source {
        let s = &self.streams[index as usize];
        match s.kind {
            Some(SourceType::Monitor) => Source::Display {
                peer: self.peer,
                display_index: index,
            },
            _ => Source::Window {
                peer: self.peer,
                handle: format!("portal:{index}"),
                title: match s.size {
                    Some((w, h)) => format!("Shared window {index} ({w}×{h})"),
                    None => format!("Shared window {index}"),
                },
            },
        }
    }

    /// Resolve a [`Source`] produced by this backend back to its stream.
    fn stream_for(&self, source: &Source) -> Option<&PickedStream> {
        let index = match source {
            Source::Display { peer, display_index } if *peer == self.peer => *display_index,
            Source::Window { peer, handle, .. } if *peer == self.peer => {
                handle.strip_prefix("portal:")?.parse().ok()?
            }
            _ => return None,
        };
        self.streams.get(index as usize)
    }
}

impl CaptureBackend for PortalBackend {
    fn enumerate(&mut self) -> Result<Vec<Source>, CaptureError> {
        Ok((0..self.streams.len() as u32).map(|i| self.source_for(i)).collect())
    }

    fn pick(&mut self, multiple: bool) -> Result<Vec<Source>, CaptureError> {
        self.request(multiple, None)
    }

    fn restore(&mut self, token: &str) -> Result<Vec<Source>, CaptureError> {
        self.request(true, Some(token.to_string()))
    }

    fn restore_token(&self) -> Option<String> {
        self.restore_token.clone()
    }

    fn inject(&mut self, source: &Source, events: &[InputEvent]) -> bool {
        let Some(s) = self.stream_for(source) else { return false };
        if !s.remote {
            return false;
        }
        let Some(size) = s.size else { return false };
        let is_window = !matches!(s.kind, Some(SourceType::Monitor));
        let rect = if is_window {
            s.crop.lock().ok().and_then(|c| *c).unwrap_or((0, 0, size.0, size.1))
        } else {
            (0, 0, size.0, size.1)
        };
        // Mutter sizes a window stream as its monitor's logical size times
        // ceil(scale) and maps positions through the window's surface in
        // logical pixels, so divide by that integer scale.
        let scale = if is_window {
            self.layout
                .iter()
                .find(|m| (m.w * m.scale.ceil()) as u32 == size.0 && (m.h * m.scale.ceil()) as u32 == size.1)
                .map(|m| m.scale.ceil())
                .unwrap_or(1.0)
        } else {
            1.0
        };
        self.req_tx
            .send(PortalReq::Inject {
                session: s.session_index,
                node_id: s.node_id,
                rect,
                scale,
                events: events.to_vec(),
            })
            .is_ok()
    }

    fn close(&mut self) {
        let (reply, rx) = mpsc::channel();
        if self.req_tx.send(PortalReq::Close { reply }).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }

    fn source_size(&self, source: &Source) -> Option<(u32, u32)> {
        self.stream_for(source).and_then(|s| s.size)
    }

    fn source_screen_rect(&self, source: &Source) -> Option<(f32, f32, f32, f32)> {
        let s = self.stream_for(source)?;
        let (px, py) = s.position?;
        let (w, h) = s.size?;
        // The stream is in device pixels; the logical monitor at this position
        // tells us the scale.
        let scale = self
            .layout
            .iter()
            .find(|m| m.x == px && m.y == py)
            .map(|m| m.scale)
            .unwrap_or(1.0);
        Some((px as f32, py as f32, w as f32 / scale, h as f32 / scale))
    }

    fn screen_bounds(&self) -> Option<(f32, f32)> {
        if self.layout.is_empty() {
            return None;
        }
        let w = self.layout.iter().map(|m| m.x as f32 + m.w).fold(0.0, f32::max);
        let h = self.layout.iter().map(|m| m.y as f32 + m.h).fold(0.0, f32::max);
        Some((w, h))
    }

    fn open(&mut self, source: &Source, fps: u32) -> Result<Box<dyn CaptureSource>, CaptureError> {
        let stream = self
            .stream_for(source)
            .ok_or_else(|| CaptureError::Other(format!("{source:?} is not a picked portal stream")))?
            .clone();
        PipeWireSource::start(source.clone(), stream, fps).map(|s| Box::new(s) as Box<dyn CaptureSource>)
    }
}

// ---------------------------------------------------------------------------
// Portal thread
// ---------------------------------------------------------------------------

fn portal_thread(rx: mpsc::Receiver<PortalReq>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("portal runtime: {e}");
            return;
        }
    };
    let mut live: Vec<LiveSession> = Vec::new();
    while let Ok(req) = rx.recv() {
        match req {
            PortalReq::Pick {
                multiple,
                restore_token,
                reply,
            } => {
                let res = rt.block_on(pick(multiple, restore_token.as_deref()));
                let _ = reply.send(res.map(|(mut r, session)| {
                    r.session_index = live.len();
                    r.remote = matches!(session, LiveSession::Remote { .. });
                    live.push(session);
                    r
                }));
            }
            PortalReq::Inject {
                session,
                node_id,
                rect,
                scale,
                events,
            } => {
                if let Some(LiveSession::Remote { proxy, session, .. }) = live.get(session) {
                    rt.block_on(inject_events(proxy, session, node_id, rect, scale, &events));
                }
            }
            PortalReq::Close { reply } => {
                for s in live.drain(..) {
                    if let Err(e) = rt.block_on(s.close()) {
                        tracing::warn!("portal session close: {e}");
                    }
                }
                let _ = reply.send(());
            }
        }
    }
}

fn portal_err(e: ashpd::Error) -> CaptureError {
    match e {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => CaptureError::Denied,
        other => CaptureError::Other(format!("portal: {other}")),
    }
}

async fn pick(multiple: bool, restore_token: Option<&str>) -> Result<(PickResult, LiveSession), CaptureError> {
    let cast = Screencast::new().await.map_err(portal_err)?;
    let types = SourceType::Monitor | SourceType::Window;
    // Prefer a RemoteDesktop session carrying the screen-cast sources: the
    // portal then accepts pointer/keyboard events in stream coordinates, so
    // window captures get exact placement without knowing where the window
    // is on the desktop. Falls back to plain ScreenCast where unavailable.
    match remote_session(multiple, restore_token, types).await {
        Ok(ok) => return Ok(ok),
        Err(e) => tracing::info!("remote-desktop portal unavailable ({e}); capture only"),
    }
    let session = cast.create_session().await.map_err(portal_err)?;
    cast.select_sources(
        &session,
        CursorMode::Embedded,
        types,
        multiple,
        restore_token,
        PersistMode::ExplicitlyRevoked,
    )
    .await
    .map_err(portal_err)?
    .response()
    .map_err(portal_err)?;
    let streams = cast
        .start(&session, None)
        .await
        .map_err(portal_err)?
        .response()
        .map_err(portal_err)?;
    let fd = cast.open_pipe_wire_remote(&session).await.map_err(portal_err)?;
    let result = pick_result(streams.streams(), streams.restore_token(), fd).await;
    Ok((result, LiveSession::Cast { _proxy: cast, session }))
}

async fn remote_session(
    multiple: bool,
    restore_token: Option<&str>,
    types: ashpd::enumflags2::BitFlags<SourceType>,
) -> Result<(PickResult, LiveSession), CaptureError> {
    let rd = RemoteDesktop::new().await.map_err(portal_err)?;
    let cast = Screencast::new().await.map_err(portal_err)?;
    let session = rd.create_session().await.map_err(portal_err)?;
    rd.select_devices(
        &session,
        DeviceType::Keyboard | DeviceType::Pointer,
        restore_token,
        PersistMode::ExplicitlyRevoked,
    )
    .await
    .map_err(portal_err)?
    .response()
    .map_err(portal_err)?;
    // Persistence belongs to the remote-desktop side of the session; the
    // portal rejects persist options on SelectSources here.
    cast.select_sources(
        &session,
        CursorMode::Embedded,
        types,
        multiple,
        None,
        PersistMode::DoNot,
    )
    .await
    .map_err(portal_err)?
    .response()
    .map_err(portal_err)?;
    let devices = rd
        .start(&session, None)
        .await
        .map_err(portal_err)?
        .response()
        .map_err(portal_err)?;
    let fd = cast.open_pipe_wire_remote(&session).await.map_err(portal_err)?;
    let streams = devices.streams().unwrap_or(&[]);
    let result = pick_result(streams, devices.restore_token(), fd).await;
    tracing::info!(devices = ?devices.devices(), "remote-desktop session: input goes through the portal");
    Ok((
        result,
        LiveSession::Remote {
            _cast: cast,
            proxy: rd,
            session,
        },
    ))
}

async fn pick_result(streams: &[ashpd::desktop::screencast::Stream], token: Option<&str>, fd: OwnedFd) -> PickResult {
    let picked = streams
        .iter()
        .map(|s| RawStream {
            node_id: s.pipe_wire_node_id(),
            size: s.size().map(|(w, h)| (w.max(1) as u32, h.max(1) as u32)),
            kind: s.source_type(),
            portal_id: s.id().map(ToOwned::to_owned),
            position: s.position(),
        })
        .collect();
    let layout = match monitor_layout().await {
        Ok(l) => l,
        Err(e) => {
            tracing::info!("monitor layout unavailable ({e}); absolute pointer mapping off");
            Vec::new()
        }
    };
    PickResult {
        streams: picked,
        layout,
        restore_token: token.map(ToOwned::to_owned),
        fd,
        session_index: 0,
        remote: false,
    }
}

/// Translate Perch input events into RemoteDesktop portal notifications.
async fn inject_events(
    rd: &RemoteDesktop<'static>,
    session: &Session<'static, RemoteDesktop<'static>>,
    node_id: u32,
    rect: crate::Crop,
    scale: f32,
    events: &[InputEvent],
) {
    let scale = scale.max(1.0);
    for ev in events {
        let res = match *ev {
            InputEvent::MouseMoveAbs { x, y } => {
                let px = ((rect.0 as f32 + x.clamp(0.0, 1.0) * rect.2 as f32) / scale) as f64;
                let py = ((rect.1 as f32 + y.clamp(0.0, 1.0) * rect.3 as f32) / scale) as f64;
                rd.notify_pointer_motion_absolute(session, node_id, px, py).await
            }
            InputEvent::MouseMove { dx, dy } => rd.notify_pointer_motion(session, dx as f64, dy as f64).await,
            InputEvent::MouseButton { button, down } => {
                let code = match button {
                    MouseButton::Left => 0x110,
                    MouseButton::Right => 0x111,
                    MouseButton::Middle => 0x112,
                    MouseButton::Back => 0x113,
                    MouseButton::Forward => 0x114,
                };
                rd.notify_pointer_button(session, code, key_state(down)).await
            }
            InputEvent::Scroll { dx, dy } => {
                // Perch: positive = up/left. Portal discrete steps: positive = down/right.
                let mut r = Ok(());
                if dy.round() != 0.0 {
                    r = rd
                        .notify_pointer_axis_discrete(session, Axis::Vertical, -(dy.round() as i32))
                        .await;
                }
                if r.is_ok() && dx.round() != 0.0 {
                    r = rd
                        .notify_pointer_axis_discrete(session, Axis::Horizontal, -(dx.round() as i32))
                        .await;
                }
                r
            }
            InputEvent::Key { usage, down, .. } => match perch_input::keymap::hid_to_evdev(usage) {
                Some(code) => rd.notify_keyboard_keycode(session, code as i32, key_state(down)).await,
                None => Ok(()),
            },
            // Controllers are not a RemoteDesktop concept; the uinput sink handles them.
            InputEvent::Gamepad { .. } => Ok(()),
            InputEvent::Text { ch } => {
                let sym = if (ch as u32) < 0x100 {
                    ch as i32
                } else {
                    (0x0100_0000 | ch as u32) as i32
                };
                match rd.notify_keyboard_keysym(session, sym, KeyState::Pressed).await {
                    Ok(()) => rd.notify_keyboard_keysym(session, sym, KeyState::Released).await,
                    e => e,
                }
            }
        };
        if let Err(e) = res {
            tracing::debug!("portal inject: {e}");
        }
    }
}

fn key_state(down: bool) -> KeyState {
    if down {
        KeyState::Pressed
    } else {
        KeyState::Released
    }
}

/// GNOME's monitor layout via `org.gnome.Mutter.DisplayConfig.GetCurrentState`.
/// Other compositors: empty (TODO wlr-output-management).
pub(crate) async fn monitor_layout() -> Result<Vec<LogicalMonitor>, String> {
    use ashpd::zbus;
    use std::collections::HashMap;
    type Props = HashMap<String, zbus::zvariant::OwnedValue>;
    type MonitorId = (String, String, String, String);
    type Mode = (String, i32, i32, f64, f64, Vec<f64>, Props);
    type Monitor = (MonitorId, Vec<Mode>, Props);
    type Logical = (i32, i32, f64, u32, bool, Vec<MonitorId>, Props);

    let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
    let reply = conn
        .call_method(
            Some("org.gnome.Mutter.DisplayConfig"),
            "/org/gnome/Mutter/DisplayConfig",
            Some("org.gnome.Mutter.DisplayConfig"),
            "GetCurrentState",
            &(),
        )
        .await
        .map_err(|e| e.to_string())?;
    let body = reply.body();
    let (_serial, monitors, logicals, _props): (u32, Vec<Monitor>, Vec<Logical>, Props) =
        body.deserialize().map_err(|e| e.to_string())?;
    let current_mode = |id: &MonitorId| -> Option<(i32, i32)> {
        monitors.iter().find(|m| &m.0 == id).and_then(|m| {
            m.1.iter()
                .find(|mode| {
                    mode.6
                        .get("is-current")
                        .and_then(|v| bool::try_from(v.clone()).ok())
                        .unwrap_or(false)
                })
                .map(|mode| (mode.1, mode.2))
        })
    };
    Ok(logicals
        .iter()
        .filter_map(|l| {
            let id = l.5.first()?;
            let (mw, mh) = current_mode(id)?;
            let scale = l.2 as f32;
            Some(LogicalMonitor {
                connector: id.0.clone(),
                x: l.0,
                y: l.1,
                w: mw as f32 / scale,
                h: mh as f32 / scale,
                scale,
            })
        })
        .collect())
}

// ---------------------------------------------------------------------------
// One PipeWire connection per portal session
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) latest: Option<Frame>,
    pub(crate) seq: u64,
    pub(crate) size: (u32, u32),
    pub(crate) ended: bool,
}

pub(crate) struct Slot {
    pub(crate) m: Mutex<Shared>,
    pub(crate) cv: Condvar,
}

impl Slot {
    pub(crate) fn end(&self) {
        let mut s = self.m.lock().unwrap();
        s.ended = true;
        self.cv.notify_all();
    }
}

enum PwCmd {
    Connect {
        node_id: u32,
        fps: u32,
        slot: Arc<Slot>,
        /// Window stream: crop to the opaque window and publish the crop.
        crop: Option<Arc<Mutex<Option<crate::Crop>>>>,
        /// Negotiate exactly this size (virtual monitors take theirs from it).
        fixed: Option<(u32, u32)>,
        reply: mpsc::Sender<Result<u64, String>>,
    },
    Disconnect(u64),
    Quit,
}

/// A PipeWire main-loop thread connected to one portal session's socket.
/// Streams are created and destroyed on that thread via [`PwCmd`].
pub struct PwSession {
    /// For `Debug` on [`PickedStream`].
    id: u64,
    cmd: pipewire::channel::Sender<PwCmd>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl PwSession {
    pub(crate) fn start(fd: OwnedFd) -> Result<Self, CaptureError> {
        Self::start_with(Some(fd))
    }

    /// Connect to the user's default PipeWire socket (Mutter-native streams).
    pub(crate) fn start_default() -> Result<Self, CaptureError> {
        Self::start_with(None)
    }

    fn start_with(fd: Option<OwnedFd>) -> Result<Self, CaptureError> {
        let (cmd_tx, cmd_rx) = pipewire::channel::channel::<PwCmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread = std::thread::Builder::new()
            .name("perch-pipewire".into())
            .spawn(move || {
                if let Err(e) = pw_session_loop(fd, cmd_rx, ready_tx) {
                    tracing::warn!("pipewire session: {e}");
                }
            })
            .map_err(|e| CaptureError::Other(format!("spawn pipewire thread: {e}")))?;
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| CaptureError::Other("pipewire connection did not come up".into()))?
            .map_err(CaptureError::Other)?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Ok(Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            cmd: cmd_tx,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub(crate) fn connect(
        &self,
        node_id: u32,
        fps: u32,
        slot: Arc<Slot>,
        crop: Option<Arc<Mutex<Option<crate::Crop>>>>,
        fixed: Option<(u32, u32)>,
    ) -> Result<u64, CaptureError> {
        let (reply, rx) = mpsc::channel();
        self.cmd
            .send(PwCmd::Connect {
                node_id,
                fps,
                slot,
                crop,
                fixed,
                reply,
            })
            .map_err(|_| CaptureError::Ended)?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| CaptureError::Other("pipewire stream did not start".into()))?
            .map_err(CaptureError::Other)
    }
}

impl std::fmt::Debug for PwSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PwSession#{}", self.id)
    }
}

impl Drop for PwSession {
    fn drop(&mut self) {
        let _ = self.cmd.send(PwCmd::Quit);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

/// A running capture of one portal stream.
pub struct PipeWireSource {
    source: Source,
    slot: Arc<Slot>,
    seen: u64,
    key: u64,
    session: Arc<PwSession>,
}

impl PipeWireSource {
    fn start(source: Source, stream: PickedStream, fps: u32) -> Result<Self, CaptureError> {
        let slot = Arc::new(Slot {
            m: Mutex::new(Shared {
                size: stream.size.unwrap_or((1920, 1080)),
                ..Default::default()
            }),
            cv: Condvar::new(),
        });
        let crop = (!matches!(stream.kind, Some(SourceType::Monitor))).then(|| Arc::clone(&stream.crop));
        let key = stream
            .session
            .connect(stream.node_id, fps, Arc::clone(&slot), crop, None)?;
        Ok(Self {
            source,
            slot,
            seen: 0,
            key,
            session: stream.session,
        })
    }
}

impl PwSession {
    /// Tear down one stream.
    pub(crate) fn disconnect(&self, key: u64) {
        let _ = self.cmd.send(PwCmd::Disconnect(key));
    }
}

impl Drop for PipeWireSource {
    fn drop(&mut self) {
        let _ = self.session.cmd.send(PwCmd::Disconnect(self.key));
    }
}

impl CaptureSource for PipeWireSource {
    fn source(&self) -> &Source {
        &self.source
    }

    fn next_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CaptureError> {
        let mut s = self.slot.m.lock().unwrap();
        if s.seq == self.seen && !s.ended && !timeout.is_zero() {
            let (guard, _) = self.slot.cv.wait_timeout(s, timeout).unwrap();
            s = guard;
        }
        if s.ended {
            return Err(CaptureError::Ended);
        }
        if s.seq != self.seen {
            self.seen = s.seq;
            return Ok(s.latest.clone());
        }
        Ok(None)
    }

    fn size(&self) -> (u32, u32) {
        self.slot.m.lock().unwrap().size
    }
}

struct UserData {
    format: pipewire::spa::param::video::VideoInfoRaw,
    slot: Arc<Slot>,
    frames: u64,
    node_id: u32,
    /// Window streams: where to publish the detected window rectangle.
    crop_out: Option<Arc<Mutex<Option<crate::Crop>>>>,
    last_crop: Option<crate::Crop>,
}

type LiveStream = (pipewire::stream::Stream, pipewire::stream::StreamListener<UserData>);

fn pw_session_loop(
    fd: Option<OwnedFd>,
    cmd_rx: pipewire::channel::Receiver<PwCmd>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    use pipewire as pw;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| e.to_string())?;
    let core = match fd {
        Some(fd) => context.connect_fd(fd, None),
        None => context.connect(None),
    };
    let core = match core {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(format!("pipewire connect: {e}")));
            return Err(e.to_string());
        }
    };
    let streams: Rc<RefCell<HashMap<u64, LiveStream>>> = Rc::new(RefCell::new(HashMap::new()));
    let next_key = std::cell::Cell::new(1u64);
    let weak = mainloop.downgrade();
    let streams2 = Rc::clone(&streams);
    let _attached = cmd_rx.attach(mainloop.loop_(), move |cmd| match cmd {
        PwCmd::Connect {
            node_id,
            fps,
            slot,
            crop,
            fixed,
            reply,
        } => {
            let res = connect_stream(&core, node_id, fps, slot, crop, fixed).map(|live| {
                let key = next_key.get();
                next_key.set(key + 1);
                streams2.borrow_mut().insert(key, live);
                key
            });
            let _ = reply.send(res);
        }
        PwCmd::Disconnect(key) => {
            if let Some((stream, _listener)) = streams2.borrow_mut().remove(&key) {
                let _ = stream.disconnect();
            }
        }
        PwCmd::Quit => {
            if let Some(l) = weak.upgrade() {
                l.quit();
            }
        }
    });
    let _ = ready.send(Ok(()));
    mainloop.run();
    for (_, (stream, _)) in streams.borrow_mut().drain() {
        let _ = stream.disconnect();
    }
    Ok(())
}

fn connect_stream(
    core: &pipewire::core::Core,
    node_id: u32,
    fps: u32,
    slot: Arc<Slot>,
    crop_out: Option<Arc<Mutex<Option<crate::Crop>>>>,
    fixed: Option<(u32, u32)>,
) -> Result<LiveStream, String> {
    let (size_def, size_min, size_max) = match fixed {
        Some((w, h)) => ((w, h), (w, h), (w, h)),
        None => ((1920, 1080), (1, 1), (8192, 8192)),
    };
    use pipewire as pw;
    use pw::spa;
    use spa::pod::{Pod, Value};

    let stream = pw::stream::Stream::new(
        core,
        "perch-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| e.to_string())?;

    let data = UserData {
        format: Default::default(),
        slot,
        crop_out,
        last_crop: None,
        frames: 0,
        node_id,
    };
    let listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, ud, old, new| {
            tracing::debug!(node_id = ud.node_id, ?old, ?new, "pipewire stream state");
            if matches!(
                new,
                pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected
            ) && !matches!(old, pw::stream::StreamState::Unconnected)
            {
                ud.slot.end();
            }
        })
        .param_changed(|_, ud, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((mt, mst)) = spa::param::format_utils::parse_format(param) else {
                return;
            };
            if mt != spa::param::format::MediaType::Video || mst != spa::param::format::MediaSubtype::Raw {
                return;
            }
            if ud.format.parse(param).is_err() {
                tracing::warn!("could not parse negotiated video format");
                return;
            }
            let size = ud.format.size();
            tracing::info!(
                node_id = ud.node_id,
                format = ?ud.format.format(),
                width = size.width,
                height = size.height,
                "capture format negotiated"
            );
            let mut s = ud.slot.m.lock().unwrap();
            s.size = (size.width, size.height);
        })
        .process(|stream, ud| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(d) = datas.first_mut() else { return };
            let size = ud.format.size();
            let (w, h) = (size.width, size.height);
            if w == 0 || h == 0 {
                return;
            }
            let chunk = d.chunk();
            let (offset, stride, len) = (
                chunk.offset() as usize,
                chunk.stride().max(0) as usize,
                chunk.size() as usize,
            );
            if len == 0 || stride < (w as usize) * 4 {
                return;
            }
            let Some(bytes) = d.data() else {
                // DMA-BUF without a mapping; we never offer modifiers so this
                // should not happen.
                return;
            };
            let (pix, opaque) = match ud.format.format() {
                spa::param::video::VideoFormat::BGRA => (PixelFormat::Bgra8, false),
                spa::param::video::VideoFormat::BGRx => (PixelFormat::Bgra8, true),
                spa::param::video::VideoFormat::RGBA => (PixelFormat::Rgba8, false),
                spa::param::video::VideoFormat::RGBx => (PixelFormat::Rgba8, true),
                other => {
                    if ud.frames == 0 {
                        tracing::warn!(?other, "unsupported capture pixel format");
                    }
                    ud.frames += 1;
                    return;
                }
            };
            let row = (w as usize) * 4;
            let mut out = vec![0u8; row * h as usize];
            for y in 0..h as usize {
                let src_start = offset + y * stride;
                let Some(src) = bytes.get(src_start..src_start + row) else {
                    break;
                };
                let dst = &mut out[y * row..(y + 1) * row];
                dst.copy_from_slice(src);
                if opaque {
                    for px in dst.as_chunks_mut::<4>().0 {
                        px[3] = 255;
                    }
                }
            }
            if ud.frames == 0 {
                tracing::info!(node_id = ud.node_id, width = w, height = h, "first capture frame");
            }
            ud.frames += 1;
            let (out, w, h, factor) = if w > crate::MAX_UPLOAD_WIDTH {
                let (o, w2, h2) = crate::downscale2x(&out, w, h, row);
                (o, w2, h2, 2u32)
            } else {
                (out, w, h, 1u32)
            };
            let row = (w as usize) * 4;
            // Window streams are monitor-sized; show just the window.
            let mut crop_px: Option<crate::Crop> = None;
            let (out, w, h) = match (&ud.crop_out, opaque) {
                (Some(crop_out), false) => match crate::opaque_bbox(&out, w, h, 24) {
                    Some(b) => {
                        let full = (b.0 * factor, b.1 * factor, b.2 * factor, b.3 * factor);
                        let full = crate::stabilise_crop(ud.last_crop, full, 3 * factor);
                        ud.last_crop = Some(full);
                        if let Ok(mut c) = crop_out.lock() {
                            *c = Some(full);
                        }
                        crop_px = Some(full);
                        let local = (full.0 / factor, full.1 / factor, full.2 / factor, full.3 / factor);
                        let local = (
                            local.0.min(w.saturating_sub(1)),
                            local.1.min(h.saturating_sub(1)),
                            local.2.clamp(1, w - local.0.min(w.saturating_sub(1))),
                            local.3.clamp(1, h - local.1.min(h.saturating_sub(1))),
                        );
                        (crate::crop_frame(&out, row, local), local.2, local.3)
                    }
                    None => (out, w, h),
                },
                _ => (out, w, h),
            };
            let row = (w as usize) * 4;
            let frame = Frame::Rgba {
                width: w,
                height: h,
                stride: row as u32,
                format: pix,
                data: out.into(),
                t_us: now_us(),
            };
            let mut s = ud.slot.m.lock().unwrap();
            if let Some(c) = crop_px {
                s.size = (c.2, c.3);
            }
            s.latest = Some(frame);
            s.seq += 1;
            ud.slot.cv.notify_all();
        })
        .register()
        .map_err(|e| e.to_string())?;

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: size_def.0,
                height: size_def.1
            },
            spa::utils::Rectangle {
                width: size_min.0,
                height: size_min.1
            },
            spa::utils::Rectangle {
                width: size_max.0,
                height: size_max.1
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: fps.max(1),
                denom: 1
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 1000, denom: 1 }
        ),
    );
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(obj))
        .map_err(|e| format!("serialize format pod: {e:?}"))?
        .0
        .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| "bad format pod".to_string())?];

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|e| e.to_string())?;
    Ok((stream, listener))
}
