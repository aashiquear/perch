//! GNOME-native backend: every spatial window is a **virtual monitor**.
//!
//! Mutter's session-bus APIs (the ones gnome-remote-desktop uses, no dialog
//! for a session app) give us:
//!
//! * `org.gnome.Mutter.ScreenCast` → `RecordVirtual`: a PipeWire stream whose
//!   negotiated size becomes a new logical monitor in the desktop layout.
//!   Windows moved there exist only on the glasses; when the stream (or the
//!   daemon) goes away the monitor vanishes and GNOME moves them back.
//! * `org.gnome.Mutter.RemoteDesktop`: pointer positions *per stream*, so the
//!   spatial pointer lands exactly inside the virtual monitor it hovers, plus
//!   keyboard and buttons. No `/dev/uinput` needed for injection.
//!
//! "Pick" here means *send the focused window to a new glasses display*: a
//! virtual monitor is created to the right of the layout and GNOME's own
//! `move-to-monitor-right` shortcut is injected the required number of times.

use crate::linux::{monitor_layout, LogicalMonitor, PwSession, Shared, Slot};
use crate::{CaptureBackend, CaptureError, CaptureSource, Frame};
use ashpd::zbus;
use perch_proto::{InputEvent, MouseButton, Source};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;
use uuid::Uuid;

const RD: &str = "org.gnome.Mutter.RemoteDesktop";
const SC: &str = "org.gnome.Mutter.ScreenCast";
/// Default virtual monitor size. The glasses panel is 1920 px wide, so one
/// window at this size maps about 1:1 when it fills the view.
pub const DEFAULT_VIRTUAL_SIZE: (u32, u32) = (1920, 1080);
const VIRTUAL_FPS: u32 = 60;
// evdev key codes for GNOME's default shortcuts.
const KEY_F4: u32 = 62;
const KEY_LEFT: u32 = 105;
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTALT: u32 = 56;
const KEY_F11: u32 = 87;
const KEY_RIGHT: u32 = 106;
const KEY_LEFTMETA: u32 = 125;
/// GNOME's `maximize` action is unbound by default; Perch binds it to this
/// deliberately obscure chord so a window can be maximized idempotently
/// (`toggle-maximized` would un-maximize one that already was).
const MAXIMIZE_BINDING: &str = "<Ctrl><Super><Alt>F11";

enum Req {
    Create {
        reply: mpsc::Sender<Result<Created, String>>,
    },
    Inject {
        display: usize,
        events: Vec<InputEvent>,
    },
    /// Press `codes` together (down in order, up in reverse), `times` times.
    Chord {
        display: usize,
        codes: Vec<u32>,
        times: usize,
    },
    Layout {
        reply: mpsc::Sender<Vec<LogicalMonitor>>,
    },
    /// Stop one session (its virtual monitor disappears).
    Stop {
        display: usize,
    },
    /// Release every key still held through any session.
    ReleaseKeys,
    Close {
        reply: mpsc::Sender<()>,
    },
}

struct Created {
    index: usize,
    stream_path: String,
    node_id: u32,
}

/// One virtual monitor shown as one spatial window.
struct VirtualDisplay {
    /// Index into the D-Bus thread's session list.
    index: usize,
    node_id: u32,
    size: (u32, u32),
    /// Logical layout rectangle once Mutter has placed the monitor.
    rect: Option<(i32, i32, f32, f32)>,
    connector: Option<String>,
    /// Shared frame slot; the backend owns the PipeWire stream so the
    /// monitor outlives any one renderer connection.
    slot: Arc<Slot>,
    pw_key: u64,
}

/// Virtual-monitor backend for GNOME/Mutter.
pub struct GnomeBackend {
    peer: Uuid,
    req_tx: mpsc::Sender<Req>,
    pw: Arc<PwSession>,
    /// Slot per display index; `None` once released (indices stay stable
    /// because [`Source::Display`] carries the index).
    displays: Vec<Option<VirtualDisplay>>,
    layout: Vec<LogicalMonitor>,
    /// Size for the next virtual display.
    pub virtual_size: (u32, u32),
    /// Connector of the glasses panel (kept out of the move row).
    glasses_connector: Option<String>,
    /// Maximize a window after sending it, so it fills its display and no
    /// desktop background shows around it.
    pub auto_maximize: bool,
    maximize_bound: bool,
}

impl GnomeBackend {
    /// Fails when Mutter's RemoteDesktop service is not on the session bus.
    pub fn new(peer: Uuid) -> Result<Self, CaptureError> {
        let (req_tx, req_rx) = mpsc::channel::<Req>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("perch-mutter".into())
            .spawn(move || dbus_thread(req_rx, ready_tx))
            .map_err(|e| CaptureError::Other(format!("spawn mutter thread: {e}")))?;
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| CaptureError::Unavailable("mutter thread did not start".into()))?
            .map_err(CaptureError::Unavailable)?;
        let pw = Arc::new(PwSession::start_default()?);
        Ok(Self {
            peer,
            req_tx,
            pw,
            displays: Vec::new(),
            layout: Vec::new(),
            virtual_size: DEFAULT_VIRTUAL_SIZE,
            glasses_connector: None,
            auto_maximize: true,
            maximize_bound: false,
        })
    }

    fn layout_now(&self) -> Vec<LogicalMonitor> {
        let (reply, rx) = mpsc::channel();
        if self.req_tx.send(Req::Layout { reply }).is_err() {
            return Vec::new();
        }
        rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default()
    }

    fn display_for(&self, source: &Source) -> Option<&VirtualDisplay> {
        match source {
            Source::Display { peer, display_index } if *peer == self.peer => {
                self.displays.get(*display_index as usize).and_then(|d| d.as_ref())
            }
            _ => None,
        }
    }

    fn source_for(&self, i: usize) -> Source {
        Source::Display {
            peer: self.peer,
            display_index: i as u32,
        }
    }

    /// Create a virtual monitor and wait for Mutter to place it.
    fn create_display(&mut self) -> Result<usize, CaptureError> {
        let before = self.layout_now();
        let (reply, rx) = mpsc::channel();
        self.req_tx
            .send(Req::Create { reply })
            .map_err(|_| CaptureError::Other("mutter thread gone".into()))?;
        let created = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| CaptureError::Other("mutter session timed out".into()))?
            .map_err(CaptureError::Other)?;
        let slot = Arc::new(Slot {
            m: Mutex::new(Shared {
                size: self.virtual_size,
                ..Default::default()
            }),
            cv: Condvar::new(),
        });
        let pw_key = self.pw.connect(
            created.node_id,
            VIRTUAL_FPS,
            Arc::clone(&slot),
            None,
            Some(self.virtual_size),
        )?;
        // The monitor appears once the stream format is negotiated.
        let mut rect = None;
        let mut connector = None;
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            let now = self.layout_now();
            if let Some(m) = now.iter().find(|m| !before.iter().any(|b| b.connector == m.connector)) {
                rect = Some((m.x, m.y, m.w, m.h));
                connector = Some(m.connector.clone());
                self.layout = now;
                break;
            }
        }
        if rect.is_none() {
            tracing::warn!("virtual monitor did not show up in the layout; pointer mapping may be off");
        }
        // Mutter lays a changed monitor set out in one row, glasses panel
        // included; put the panel back under the laptop so window moves go
        // laptop → virtual displays directly, then re-read positions.
        if self.arrange_layout() {
            let now = self.layout_now();
            if let Some(m) = connector.as_ref().and_then(|c| now.iter().find(|m| &m.connector == c)) {
                rect = Some((m.x, m.y, m.w, m.h));
            }
            self.layout = now;
        }
        tracing::info!(
            index = self.displays.len(),
            node_id = created.node_id,
            ?connector,
            ?rect,
            "virtual glasses display created"
        );
        // Re-read every display's rectangle: arranging moves them.
        for d in self.displays.iter_mut().flatten() {
            if let Some(m) = d
                .connector
                .as_ref()
                .and_then(|c| self.layout.iter().find(|m| &m.connector == c))
            {
                d.rect = Some((m.x, m.y, m.w, m.h));
            }
        }
        self.displays.push(Some(VirtualDisplay {
            index: created.index,
            node_id: created.node_id,
            size: self.virtual_size,
            rect,
            connector,
            slot,
            pw_key,
        }));
        let _ = created.stream_path;
        Ok(self.displays.len() - 1)
    }

    /// Desktop layout Perch wants: laptop at the origin, the glasses panel
    /// *below* it, virtual displays chained to the right of the laptop.
    /// Window moves hop monitor by monitor, so with the glasses panel in the
    /// row every move would cross it (and a missed step leaves the app on
    /// the glasses' own output). Returns true when a change was applied.
    fn arrange_layout(&mut self) -> bool {
        let Some(glasses) = self.glasses_connector.clone() else {
            return false;
        };
        let layout = self.layout_now();
        let laptop_x = layout
            .iter()
            .filter(|m| m.connector.starts_with("eDP") || (m.x == 0 && m.y == 0))
            .map(|m| m.x)
            .min()
            .unwrap_or(0);
        let Some(laptop) = layout.iter().find(|m| m.x == laptop_x && m.y == 0).cloned() else {
            return false;
        };
        let Some(panel) = layout.iter().find(|m| m.connector == glasses).cloned() else {
            return false;
        };
        let mut virtuals: Vec<&LogicalMonitor> = layout
            .iter()
            .filter(|m| m.connector != laptop.connector && m.connector != glasses)
            .collect();
        virtuals.sort_by_key(|m| m.x);
        // Already as wanted?
        let chained = virtuals
            .iter()
            .scan(laptop.x + laptop.w as i32, |next, m| {
                let ok = m.x == *next && m.y == laptop.y;
                *next = m.x + m.w as i32;
                Some(ok)
            })
            .all(|ok| ok);
        if panel.y == laptop.y + laptop.h as i32 && panel.x == laptop.x && chained {
            return false;
        }
        let mut args: Vec<String> = vec!["set".into()];
        args.extend(["--logical-monitor", "--primary", "--monitor", &laptop.connector].map(String::from));
        args.extend(["--logical-monitor", "--monitor", &glasses, "--below", &laptop.connector].map(String::from));
        let mut prev = laptop.connector.clone();
        for m in virtuals {
            args.extend(["--logical-monitor", "--monitor", &m.connector, "--right-of", &prev].map(String::from));
            prev = m.connector.clone();
        }
        match std::process::Command::new("gdctl").args(&args).status() {
            Ok(st) if st.success() => {
                tracing::info!(
                    glasses,
                    "arranged desktop layout: glasses panel below the laptop, displays to the right"
                );
                std::thread::sleep(Duration::from_millis(400));
                true
            }
            other => {
                tracing::warn!("could not arrange monitors with gdctl: {other:?}");
                false
            }
        }
    }

    /// Click the header bar of the app on display `i` so it has focus before
    /// a shortcut is sent (shortcuts go to the focused window, which could
    /// otherwise be anything, including Perch's own windows).
    fn focus_app_on(&self, i: usize) {
        let Some(Some(d)) = self.displays.get(i) else { return };
        let (w, _) = (d.size.0 as f32, d.size.1 as f32);
        let _ = self.req_tx.send(Req::Inject {
            display: d.index,
            events: vec![
                InputEvent::MouseMoveAbs { x: w / 2.0, y: 12.0 },
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    down: true,
                },
                InputEvent::MouseButton {
                    button: MouseButton::Left,
                    down: false,
                },
            ],
        });
        std::thread::sleep(Duration::from_millis(150));
    }

    /// X of the laptop display (origin of window moves).
    fn laptop_x(&self) -> i32 {
        self.layout
            .iter()
            .filter(|m| m.connector.starts_with("eDP") || (m.x == 0 && m.y == 0))
            .map(|m| m.x)
            .min()
            .unwrap_or(0)
    }

    /// Move the focused window from the display column at `from_x` onto
    /// display `i` with GNOME's `move-to-monitor-left/right` shortcuts, one
    /// per monitor column in between.
    fn move_focused_window_to(&mut self, i: usize, from_x: i32) {
        let Some(Some(d)) = self.displays.get(i) else { return };
        let Some((x, _, _, _)) = d.rect else { return };
        let d_index = d.index;
        let mut xs: Vec<i32> = self.layout.iter().map(|m| m.x).collect();
        xs.sort_unstable();
        xs.dedup();
        let from = xs.iter().position(|&v| v == from_x).unwrap_or(0);
        let to = xs.iter().position(|&v| v == x).unwrap_or(from);
        if from == to {
            return;
        }
        let (steps, key) = if to > from {
            (to - from, KEY_RIGHT)
        } else {
            (from - to, KEY_LEFT)
        };
        let _ = self.req_tx.send(Req::Chord {
            display: d_index,
            codes: vec![KEY_LEFTMETA, KEY_LEFTSHIFT, key],
            times: steps,
        });
        if self.auto_maximize && self.ensure_maximize_binding() {
            let _ = self.req_tx.send(Req::Chord {
                display: d_index,
                codes: vec![KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTALT, KEY_F11],
                times: 1,
            });
        }
    }

    /// Bind GNOME's `maximize` action (unbound by default) once per run.
    fn ensure_maximize_binding(&mut self) -> bool {
        if self.maximize_bound {
            return true;
        }
        let get = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.wm.keybindings", "maximize"])
            .output();
        let current = get
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if current.contains(MAXIMIZE_BINDING) {
            self.maximize_bound = true;
            return true;
        }
        if !(current.is_empty() || current == "@as []" || current == "[]") {
            tracing::info!(
                current,
                "leaving the user's GNOME `maximize` binding alone; no auto-maximize"
            );
            return false;
        }
        let set = std::process::Command::new("gsettings")
            .args([
                "set",
                "org.gnome.desktop.wm.keybindings",
                "maximize",
                &format!("['{MAXIMIZE_BINDING}']"),
            ])
            .status();
        self.maximize_bound = matches!(set, Ok(s) if s.success());
        if self.maximize_bound {
            tracing::info!(
                binding = MAXIMIZE_BINDING,
                "bound GNOME `maximize` for auto-maximize on send"
            );
        }
        self.maximize_bound
    }
}

impl CaptureBackend for GnomeBackend {
    fn enumerate(&mut self) -> Result<Vec<Source>, CaptureError> {
        Ok((0..self.displays.len())
            .filter(|&i| self.displays[i].is_some())
            .map(|i| self.source_for(i))
            .collect())
    }

    fn open(&mut self, source: &Source, _fps: u32) -> Result<Box<dyn CaptureSource>, CaptureError> {
        let d = self
            .display_for(source)
            .ok_or_else(|| CaptureError::Other("unknown virtual display".into()))?;
        Ok(Box::new(SlotSource {
            source: source.clone(),
            slot: Arc::clone(&d.slot),
            seen: 0,
        }))
    }

    fn create_virtual_display(&mut self, width: u32, height: u32) -> Result<Source, CaptureError> {
        self.virtual_size = (width.max(640), height.max(480));
        let i = self.create_display()?;
        Ok(self.source_for(i))
    }

    /// Send the focused window to a new glasses display.
    fn pick(&mut self, _multiple: bool) -> Result<Vec<Source>, CaptureError> {
        let i = self.create_display()?;
        let from = self.laptop_x();
        self.move_focused_window_to(i, from);
        Ok(vec![self.source_for(i)])
    }

    fn move_to_glasses(&mut self, width: u32, height: u32) -> Result<Vec<Source>, CaptureError> {
        self.virtual_size = (width, height);
        self.pick(false)
    }

    fn close_app(&mut self, source: &Source) {
        let Some(d) = self.display_for(source) else { return };
        let (slot_i, d_index) = (
            match source {
                Source::Display { display_index, .. } => *display_index as usize,
                _ => return,
            },
            d.index,
        );
        self.focus_app_on(slot_i);
        let _ = self.req_tx.send(Req::Chord {
            display: d_index,
            codes: vec![KEY_LEFTALT, KEY_F4],
            times: 1,
        });
        // Let the app go before its display is torn down.
        std::thread::sleep(Duration::from_millis(400));
    }

    fn resize_display(&mut self, source: &Source, width: u32, height: u32) -> Result<Source, CaptureError> {
        let old = self
            .display_for(source)
            .ok_or_else(|| CaptureError::Other("unknown virtual display".into()))?;
        let from_x = old.rect.map(|r| r.0).unwrap_or_else(|| self.laptop_x());
        let old_slot = match source {
            Source::Display { display_index, .. } => *display_index as usize,
            _ => unreachable!("display_for matched"),
        };
        self.virtual_size = (width.max(320), height.max(240));
        let i = self.create_display()?;
        // Positions shift when a display is added; use the refreshed ones.
        let from_x = self
            .displays
            .get(old_slot)
            .and_then(|d| d.as_ref())
            .and_then(|d| d.rect.map(|r| r.0))
            .unwrap_or(from_x);
        self.focus_app_on(old_slot);
        self.move_focused_window_to(i, from_x);
        // The chord runs on the D-Bus thread; give it time before the old
        // display disappears under the window.
        std::thread::sleep(Duration::from_millis(600));
        self.release(source);
        Ok(self.source_for(i))
    }

    fn set_glasses_connector(&mut self, connector: &str) {
        self.glasses_connector = Some(connector.to_string());
        self.layout = self.layout_now();
        if self.arrange_layout() {
            self.layout = self.layout_now();
        }
    }

    fn restore(&mut self, _token: &str) -> Result<Vec<Source>, CaptureError> {
        Err(CaptureError::Unavailable(
            "virtual displays are not persisted; send windows again".into(),
        ))
    }

    fn source_size(&self, source: &Source) -> Option<(u32, u32)> {
        self.display_for(source).map(|d| d.size)
    }

    fn source_screen_rect(&self, source: &Source) -> Option<(f32, f32, f32, f32)> {
        let r = self.display_for(source)?.rect?;
        Some((r.0 as f32, r.1 as f32, r.2, r.3))
    }

    fn screen_bounds(&self) -> Option<(f32, f32)> {
        if self.layout.is_empty() {
            return None;
        }
        let w = self.layout.iter().map(|m| m.x as f32 + m.w).fold(0.0, f32::max);
        let h = self.layout.iter().map(|m| m.y as f32 + m.h).fold(0.0, f32::max);
        Some((w, h))
    }

    fn inject(&mut self, source: &Source, events: &[InputEvent]) -> bool {
        let Some(d) = self.display_for(source) else {
            return false;
        };
        // Window-normalised → stream pixels; the stream is the whole monitor.
        let (w, h) = (d.size.0 as f32, d.size.1 as f32);
        let events = events
            .iter()
            .map(|e| match *e {
                InputEvent::MouseMoveAbs { x, y } => InputEvent::MouseMoveAbs {
                    x: x.clamp(0.0, 1.0) * w,
                    y: y.clamp(0.0, 1.0) * h,
                },
                other => other,
            })
            .collect();
        self.req_tx
            .send(Req::Inject {
                display: d.index,
                events,
            })
            .is_ok()
    }

    fn release_pointer(&mut self) {
        let _ = self.req_tx.send(Req::ReleaseKeys);
        // Park the OS pointer in the middle of the laptop display: a huge
        // up-left motion reaches the top-left corner of the layout, then
        // half the laptop's logical size.
        let laptop_x = self.laptop_x();
        let (w, h) = self
            .layout
            .iter()
            .find(|m| m.x == laptop_x && m.y == 0)
            .map(|m| (m.w, m.h))
            .unwrap_or((2048.0, 1280.0));
        if let Some(d) = self.displays.iter().flatten().next() {
            let _ = self.req_tx.send(Req::Inject {
                display: d.index,
                events: vec![
                    InputEvent::MouseMove {
                        dx: -30000.0,
                        dy: -30000.0,
                    },
                    InputEvent::MouseMove {
                        dx: w / 2.0,
                        dy: h / 2.0,
                    },
                ],
            });
        }
    }

    fn release(&mut self, source: &Source) {
        let Source::Display { peer, display_index } = source else {
            return;
        };
        if *peer != self.peer {
            return;
        }
        let Some(slot) = self.displays.get_mut(*display_index as usize) else {
            return;
        };
        if let Some(d) = slot.take() {
            tracing::info!(connector = ?d.connector, "releasing virtual glasses display; its windows return to the laptop");
            self.pw.disconnect(d.pw_key);
            d.slot.end();
            let _ = self.req_tx.send(Req::Stop { display: d.index });
            // Mutter re-lays the remaining monitors out in a row; restore
            // the laptop / glasses-below / displays-right arrangement.
            std::thread::sleep(Duration::from_millis(400));
            self.arrange_layout();
            self.layout = self.layout_now();
            for d in self.displays.iter_mut().flatten() {
                if let Some(m) = d
                    .connector
                    .as_ref()
                    .and_then(|c| self.layout.iter().find(|m| &m.connector == c))
                {
                    d.rect = Some((m.x, m.y, m.w, m.h));
                }
            }
        }
    }

    fn close(&mut self) {
        for d in self.displays.drain(..).flatten() {
            tracing::info!(connector = ?d.connector, node_id = d.node_id, "closing virtual glasses display");
            self.pw.disconnect(d.pw_key);
            d.slot.end();
        }
        let (reply, rx) = mpsc::channel();
        if self.req_tx.send(Req::Close { reply }).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }
}

/// Reads frames from a backend-owned stream slot.
struct SlotSource {
    source: Source,
    slot: Arc<Slot>,
    seen: u64,
}

impl CaptureSource for SlotSource {
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

// ---------------------------------------------------------------------------
// D-Bus thread: one persistent connection owns every Mutter session.
// ---------------------------------------------------------------------------

struct MutterSession {
    rd: zbus::Proxy<'static>,
    stream_path: String,
    /// Keycodes currently pressed through this session's virtual keyboard.
    held: std::collections::HashSet<u32>,
}

fn dbus_thread(rx: mpsc::Receiver<Req>, ready: mpsc::Sender<Result<(), String>>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = ready.send(Err(format!("tokio: {e}")));
            return;
        }
    };
    let conn = match rt.block_on(async {
        let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.map_err(|e| e.to_string())?;
        let has = dbus
            .name_has_owner(RD.try_into().map_err(|e| format!("{e}"))?)
            .await
            .map_err(|e| e.to_string())?;
        if !has {
            return Err(format!("{RD} is not on the session bus (not GNOME?)"));
        }
        Ok(conn)
    }) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut sessions: Vec<MutterSession> = Vec::new();
    while let Ok(first) = rx.recv() {
        // Drain what else is queued so pointer moves can be coalesced.
        let mut batch = vec![first];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
        }
        let mut pending: Vec<(usize, Vec<InputEvent>)> = Vec::new();
        for req in batch {
            match req {
                Req::Inject { display, events } => match pending.last_mut() {
                    Some((d, evs)) if *d == display => evs.extend(events),
                    _ => pending.push((display, events)),
                },
                other => {
                    flush(&rt, &mut sessions, &mut pending);
                    match other {
                        Req::Create { reply } => {
                            let res = rt.block_on(create_session(&conn)).map(|(rd, stream_path, node_id)| {
                                sessions.push(MutterSession {
                                    rd,
                                    stream_path: stream_path.clone(),
                                    held: Default::default(),
                                });
                                Created {
                                    index: sessions.len() - 1,
                                    stream_path,
                                    node_id,
                                }
                            });
                            let _ = reply.send(res);
                        }
                        Req::Chord { display, codes, times } => {
                            if let Some(s) = sessions.get(display) {
                                rt.block_on(chord(&s.rd, &codes, times));
                            }
                        }
                        Req::ReleaseKeys => {
                            for s in sessions.iter_mut() {
                                let held: Vec<u32> = s.held.drain().collect();
                                for code in held {
                                    let _ = rt.block_on(s.rd.call_method("NotifyKeyboardKeycode", &(code, false)));
                                }
                            }
                        }
                        Req::Stop { display } => {
                            if let Some(s) = sessions.get(display) {
                                let _ = rt.block_on(s.rd.call_method("Stop", &()));
                            }
                        }
                        Req::Layout { reply } => {
                            let _ = reply.send(rt.block_on(monitor_layout()).unwrap_or_default());
                        }
                        Req::Close { reply } => {
                            for s in sessions.drain(..) {
                                let _ = rt.block_on(s.rd.call_method("Stop", &()));
                            }
                            let _ = reply.send(());
                            return;
                        }
                        Req::Inject { .. } => unreachable!(),
                    }
                }
            }
        }
        flush(&rt, &mut sessions, &mut pending);
    }
}

fn flush(rt: &tokio::runtime::Runtime, sessions: &mut [MutterSession], pending: &mut Vec<(usize, Vec<InputEvent>)>) {
    for (display, events) in pending.drain(..) {
        if let Some(s) = sessions.get_mut(display) {
            let events = crate::coalesce_moves(events);
            rt.block_on(inject(s, &events));
        }
    }
}

async fn proxy(
    conn: &zbus::Connection,
    dest: &'static str,
    path: String,
    iface: &'static str,
) -> Result<zbus::Proxy<'static>, String> {
    zbus::Proxy::new_owned(conn.clone(), dest, path, iface)
        .await
        .map_err(|e| e.to_string())
}

/// RemoteDesktop session + linked ScreenCast session + one virtual stream,
/// started; returns the RD session proxy, the stream path and its node id.
async fn create_session(conn: &zbus::Connection) -> Result<(zbus::Proxy<'static>, String, u32), String> {
    use futures_util::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, Value};

    let rd_root = proxy(conn, RD, "/org/gnome/Mutter/RemoteDesktop".into(), RD).await?;
    let rd_path: OwnedObjectPath = rd_root
        .call("CreateSession", &())
        .await
        .map_err(|e| format!("RemoteDesktop.CreateSession: {e}"))?;
    let rd = proxy(conn, RD, rd_path.to_string(), "org.gnome.Mutter.RemoteDesktop.Session").await?;
    let sid: String = rd.get_property("SessionId").await.map_err(|e| e.to_string())?;

    let sc_root = proxy(conn, SC, "/org/gnome/Mutter/ScreenCast".into(), SC).await?;
    let mut props: HashMap<&str, Value<'_>> = HashMap::new();
    props.insert("remote-desktop-session-id", Value::from(sid.as_str()));
    let sc_path: OwnedObjectPath = sc_root
        .call("CreateSession", &(props,))
        .await
        .map_err(|e| format!("ScreenCast.CreateSession: {e}"))?;
    let sc = proxy(conn, SC, sc_path.to_string(), "org.gnome.Mutter.ScreenCast.Session").await?;
    let mut sprops: HashMap<&str, Value<'_>> = HashMap::new();
    // 0 = hidden: Perch draws its own pointer.
    sprops.insert("cursor-mode", Value::from(0u32));
    let stream_path: OwnedObjectPath = sc
        .call("RecordVirtual", &(sprops,))
        .await
        .map_err(|e| format!("RecordVirtual: {e}"))?;
    let stream = proxy(conn, SC, stream_path.to_string(), "org.gnome.Mutter.ScreenCast.Stream").await?;
    let mut added = stream
        .receive_signal("PipeWireStreamAdded")
        .await
        .map_err(|e| e.to_string())?;
    rd.call_method("Start", &())
        .await
        .map_err(|e| format!("RemoteDesktop.Session.Start: {e}"))?;
    let msg = tokio::time::timeout(Duration::from_secs(5), added.next())
        .await
        .map_err(|_| "no PipeWireStreamAdded within 5 s".to_string())?
        .ok_or_else(|| "signal stream closed".to_string())?;
    let (node_id,): (u32,) = msg.body().deserialize().map_err(|e| e.to_string())?;
    Ok((rd, stream_path.to_string(), node_id))
}

async fn inject(s: &mut MutterSession, events: &[InputEvent]) {
    for ev in events {
        let res = match *ev {
            InputEvent::MouseMoveAbs { x, y } => {
                // Already in stream pixels (see `GnomeBackend::inject`).
                s.rd.call_noreply(
                    "NotifyPointerMotionAbsolute",
                    &(s.stream_path.as_str(), x as f64, y as f64),
                )
                .await
            }
            InputEvent::MouseMove { dx, dy } => {
                s.rd.call_noreply("NotifyPointerMotionRelative", &(dx as f64, dy as f64))
                    .await
            }
            InputEvent::MouseButton { button, down } => {
                let code: i32 = match button {
                    MouseButton::Left => 0x110,
                    MouseButton::Right => 0x111,
                    MouseButton::Middle => 0x112,
                    MouseButton::Back => 0x113,
                    MouseButton::Forward => 0x114,
                };
                s.rd.call_noreply("NotifyPointerButton", &(code, down)).await
            }
            InputEvent::Scroll { dx, dy } => {
                let mut r = Ok(());
                if dy.round() != 0.0 {
                    r =
                        s.rd.call_noreply("NotifyPointerAxisDiscrete", &(0u32, -(dy.round() as i32)))
                            .await;
                }
                if r.is_ok() && dx.round() != 0.0 {
                    r =
                        s.rd.call_noreply("NotifyPointerAxisDiscrete", &(1u32, -(dx.round() as i32)))
                            .await;
                }
                r
            }
            InputEvent::Key { usage, down, .. } => match perch_input::keymap::hid_to_evdev(usage) {
                Some(code) => {
                    if down {
                        s.held.insert(code as u32);
                    } else {
                        s.held.remove(&(code as u32));
                    }
                    s.rd.call_noreply("NotifyKeyboardKeycode", &(code as u32, down)).await
                }
                None => Ok(()),
            },
            InputEvent::Text { ch } => {
                let sym: u32 = if (ch as u32) < 0x100 {
                    ch as u32
                } else {
                    0x0100_0000 | ch as u32
                };
                match s.rd.call_noreply("NotifyKeyboardKeysym", &(sym, true)).await {
                    Ok(()) => s.rd.call_noreply("NotifyKeyboardKeysym", &(sym, false)).await,
                    e => e,
                }
            }
        };
        if let Err(e) = res {
            tracing::debug!("mutter inject: {e}");
        }
    }
}

async fn chord(rd: &zbus::Proxy<'static>, codes: &[u32], times: usize) {
    for _ in 0..times {
        for &c in codes {
            let _ = rd.call_method("NotifyKeyboardKeycode", &(c, true)).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        for &c in codes.iter().rev() {
            let _ = rd.call_method("NotifyKeyboardKeycode", &(c, false)).await;
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
