//! Wires the control server, stream socket, device thread and engine together.

use crate::capture::{SharedBackend, UiMsg};
use crate::config::Config;
use crate::device::{DeviceCommand, DeviceEvent, DeviceHandle};
use crate::engine::{Effect, Engine};
use perch_net::{ControlServer, Discovery, Inbound, StreamSocket};
use perch_proto::{
    Capabilities, Command, Event, Hello, InputEvent, Message, Platform, StreamPacket, WindowId, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

/// Build our Hello from config.
pub fn self_hello(cfg: &Config) -> Hello {
    Hello {
        version: PROTOCOL_VERSION,
        peer_id: cfg.peer_id,
        name: cfg.name.clone(),
        platform: if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        },
        caps: Capabilities {
            glasses_host: true,
            screen_source: true,
            input_sink: true,
            input_source: cfg.input.share_local_devices,
            dex: false,
        },
    }
}

/// Local input sink (uinput on Linux) — boxed so the server doesn't care.
pub type LocalSink = Option<Box<dyn perch_input::InputSink>>;

/// Result of a portal pick/restore, delivered back to the main loop.
type PickOutcome = Result<Vec<(perch_proto::Source, Option<(u32, u32)>)>, String>;

/// Result of a blocking backend job started from an effect.
enum SideOutcome {
    /// The app behind `id` was told to close; now remove the window.
    Closed(WindowId),
    /// `id`'s display was re-created with this source and size.
    Resized(WindowId, Result<(perch_proto::Source, (u32, u32)), String>),
}

/// Run a pick (or restore) on the blocking pool and report back.
fn spawn_pick(
    backend: SharedBackend,
    restore: Option<String>,
    multiple: bool,
    size: Option<(u32, u32)>,
    tx: mpsc::UnboundedSender<PickOutcome>,
) {
    tokio::task::spawn_blocking(move || {
        let res = (|| {
            let mut guard = backend.lock().map_err(|_| "capture backend poisoned".to_string())?;
            let b = guard.as_mut().ok_or_else(|| {
                "screen capture is not available on this host (build perchd with --features linux-pipewire)".to_string()
            })?;
            let sources = match restore {
                Some(tok) => b.restore(&tok),
                None => match size {
                    Some((w, h)) => b.move_to_glasses(w, h),
                    None => b.pick(multiple),
                },
            }
            .map_err(|e| e.to_string())?;
            let sized = sources
                .into_iter()
                .map(|s| {
                    let size = b.source_size(&s).filter(|(w, h)| *w > 0 && *h > 0);
                    (s, size)
                })
                .collect();
            Ok(sized)
        })();
        let _ = tx.send(res);
    });
}

/// Run everything until shutdown.
pub async fn run(
    mut cfg: Config,
    engine: Arc<Mutex<Engine>>,
    mut dev: DeviceHandle,
    mut local_sink: LocalSink,
    backend: SharedBackend,
    mut ui_rx: mpsc::UnboundedReceiver<UiMsg>,
) -> anyhow::Result<()> {
    let (server, mut inbound) = ControlServer::new(self_hello(&cfg));
    let server = if cfg.pairing_token.is_empty() {
        server
    } else {
        server.with_token(cfg.pairing_token.clone())
    };
    let server = Arc::new(server);
    let stream = Arc::new(StreamSocket::bind(cfg.stream_bind).await?);
    // Stream subscribers: peers that sent us a Ping on the UDP socket.
    let stream_peers: Arc<Mutex<HashMap<SocketAddr, std::time::Instant>>> = Default::default();

    if cfg.mdns {
        match Discovery::new() {
            Ok(d) => {
                if let Err(e) = d.advertise(&cfg.name, cfg.peer_id, cfg.control_bind.port()) {
                    tracing::warn!("mDNS advertise failed: {e}");
                }
                // Keep the daemon alive for the process lifetime.
                std::mem::forget(d);
            }
            Err(e) => tracing::warn!("mDNS unavailable: {e}"),
        }
    }

    // Control server task.
    {
        let s = Arc::clone(&server);
        let bind = cfg.control_bind;
        tokio::spawn(async move {
            if let Err(e) = s.serve(bind).await {
                tracing::error!("control server: {e}");
            }
        });
    }

    // Stream receive task: input from phones, pings for subscription.
    let (stream_in_tx, mut stream_in_rx) = mpsc::channel::<Vec<InputEvent>>(256);
    {
        let st = Arc::clone(&stream);
        let peers = Arc::clone(&stream_peers);
        tokio::spawn(async move {
            loop {
                match st.recv().await {
                    Ok((StreamPacket::Ping(n), from)) => {
                        peers.lock().await.insert(from, std::time::Instant::now());
                        let _ = st.send_to(&StreamPacket::Pong(n), from).await;
                    }
                    Ok((StreamPacket::Input(evs), _)) => {
                        let _ = stream_in_tx.send(evs).await;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("stream recv: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }

    // Pose fan-out: low-rate on control (30 Hz), full-rate on stream.
    {
        let st = Arc::clone(&stream);
        let peers = Arc::clone(&stream_peers);
        let srv = Arc::clone(&server);
        let eng = Arc::clone(&engine);
        let mut pose_rx = dev.pose_rx.clone();
        tokio::spawn(async move {
            let mut last_ctrl = std::time::Instant::now();
            loop {
                if pose_rx.changed().await.is_err() {
                    break;
                }
                let pose = *pose_rx.borrow_and_update();
                eng.lock().await.set_pose(pose);
                let now = std::time::Instant::now();
                {
                    let mut p = peers.lock().await;
                    p.retain(|_, t| now.duration_since(*t).as_secs() < 10);
                    for addr in p.keys() {
                        let _ = st.send_to(&StreamPacket::Pose(pose), *addr).await;
                    }
                }
                if now.duration_since(last_ctrl).as_millis() >= 33 {
                    last_ctrl = now;
                    srv.broadcast(Message::Event(Event::Pose(pose)));
                }
            }
        });
    }

    let button_actions = [cfg.buttons.button0.clone(), cfg.buttons.button1.clone()];
    let mut known_peers: HashMap<Uuid, Hello> = HashMap::new();

    // Capture picks run on the blocking pool and report back here.
    let (pick_tx, mut pick_rx) = mpsc::unbounded_channel::<PickOutcome>();
    let (side_tx, mut side_rx) = mpsc::unbounded_channel::<SideOutcome>();
    if !cfg.capture.restore_token.is_empty() {
        tracing::info!("restoring last screen-capture selection");
        spawn_pick(
            Arc::clone(&backend),
            Some(cfg.capture.restore_token.clone()),
            true,
            None,
            pick_tx.clone(),
        );
    }

    loop {
        tokio::select! {
            Some(Inbound { from, msg }) = inbound.recv() => {
                let fx = {
                    let mut e = engine.lock().await;
                    match msg {
                        Message::Hello(h) => e.peer_joined(h),
                        Message::Command(c) => e.command(from, c),
                        Message::Input { events } => e.input(events),
                        Message::Ping { nonce } => {
                            server.send_to(from, Message::Pong { nonce }).await;
                            vec![]
                        }
                        _ => vec![],
                    }
                };
                apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
                // Track joins/leaves by diffing the server's peer list.
                let now: HashMap<Uuid, Hello> = server.peers().await.into_iter().map(|h| (h.peer_id, h)).collect();
                for gone in known_peers.keys().filter(|k| !now.contains_key(k)).copied().collect::<Vec<_>>() {
                    let fx = engine.lock().await.peer_left(gone);
                    apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
                }
                for (id, h) in &now {
                    if !known_peers.contains_key(id) {
                        let fx = engine.lock().await.peer_joined(h.clone());
                        apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
                    }
                }
                known_peers = now;
            }
            Some(evs) = stream_in_rx.recv() => {
                let fx = engine.lock().await.input(evs);
                apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
            }
            Some(msg) = ui_rx.recv() => {
                let me = cfg.peer_id;
                match msg {
                    UiMsg::Command(cmd) => {
                        let fx = engine.lock().await.command(me, cmd);
                        apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
                    }
                    UiMsg::GrabChanged(grabbed) => {
                        if !grabbed {
                            if let Ok(mut b) = backend.try_lock() {
                                if let Some(b) = b.as_mut() {
                                    b.release_pointer();
                                }
                            }
                        }
                    }
                    UiMsg::Input { window, events } => {
                        let source = engine.lock().await.ws.get(window).map(|w| w.source.clone());
                        let Some(source) = source else { continue };
                        if perch_input::source_peer(&source) == me {
                            // The capture session can usually deliver input itself
                            // (portal RemoteDesktop), with stream-relative placement.
                            let handled = backend
                                .try_lock()
                                .ok()
                                .and_then(|mut b| b.as_mut().map(|b| b.inject(&source, &events)))
                                .unwrap_or(false);
                            if handled {
                                continue;
                            }
                            if let Some(sink) = local_sink.as_mut() {
                                let target = backend.try_lock().ok().and_then(|b| {
                                    let b = b.as_ref()?;
                                    Some(perch_input::AbsTarget {
                                        rect: b.source_screen_rect(&source)?,
                                        bounds: b.screen_bounds()?,
                                    })
                                });
                                sink.set_abs_target(target);
                                for e in events {
                                    if let Err(err) = sink.inject(e) {
                                        tracing::debug!("inject: {err}");
                                    }
                                }
                                let _ = sink.flush();
                            }
                        } else {
                            server.send_to(perch_input::source_peer(&source), Message::Input { events }).await;
                        }
                    }
                }
            }
            Some(outcome) = side_rx.recv() => {
                let fx = match outcome {
                    SideOutcome::Closed(id) => engine.lock().await.command(cfg.peer_id, Command::RemoveWindow { id }),
                    SideOutcome::Resized(id, Ok((source, px))) => engine.lock().await.replace_source(id, source, px),
                    SideOutcome::Resized(_, Err(msg)) => {
                        tracing::warn!("resize display failed: {msg}");
                        vec![Effect::Broadcast(Event::Error { message: format!("resize: {msg}") })]
                    }
                };
                apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
            }
            Some(outcome) = pick_rx.recv() => {
                let fx = match outcome {
                    Ok(picked) => {
                        let n = picked.len();
                        let mut e = engine.lock().await;
                        let mut fx = e.add_picked_windows(picked);
                        if fx.is_empty() {
                            fx.push(Effect::Broadcast(Event::Error {
                                message: format!("{n} source(s) selected but no new windows were added"),
                            }));
                        }
                        // Persist the restore token so the next start needs no dialog.
                        let token = backend.lock().ok().and_then(|b| b.as_ref().and_then(|b| b.restore_token()));
                        if let (Some(tok), Some(paths)) = (token, e.paths()) {
                            if tok != cfg.capture.restore_token {
                                cfg.capture.restore_token = tok;
                                if let Err(err) = paths.save_config(&cfg) {
                                    tracing::warn!("could not save restore token: {err}");
                                }
                            }
                        }
                        fx
                    }
                    Err(msg) => {
                        tracing::warn!("capture pick failed: {msg}");
                        if cfg.capture.restore_token.is_empty() {
                            vec![Effect::Broadcast(Event::Error { message: format!("capture: {msg}") })]
                        } else {
                            // A stale restore token: forget it so the next start doesn't retry.
                            cfg.capture.restore_token.clear();
                            if let Some(paths) = engine.lock().await.paths() {
                                let _ = paths.save_config(&cfg);
                            }
                            vec![]
                        }
                    }
                };
                apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
            }
            Some(ev) = dev.events_rx.recv() => {
                let fx = {
                    let mut e = engine.lock().await;
                    match ev {
                        DeviceEvent::Connected { model, serial, mode } => {
                            e.glasses = Some(model.clone());
                            e.display_mode = mode;
                            vec![Effect::Broadcast(Event::GlassesConnected { model, serial }), Effect::Broadcast(Event::State(e.state()))]
                        }
                        DeviceEvent::Disconnected => {
                            e.glasses = None;
                            vec![Effect::Broadcast(Event::GlassesDisconnected)]
                        }
                        DeviceEvent::Calibration(c) => {
                            e.calibration = c;
                            vec![Effect::Broadcast(Event::Calibration(c))]
                        }
                        DeviceEvent::CalibrationRejected { peak_deg_s } => vec![Effect::Broadcast(Event::Error {
                            message: format!(
                                "calibration rejected: glasses moved (peak {peak_deg_s:.1}°/s); keep them still and retry"
                            ),
                        })],
                        DeviceEvent::Button(b) => {
                            let action = button_actions.get(b as usize).cloned().unwrap_or_default();
                            e.button(&action)
                        }
                        DeviceEvent::Worn(_) => vec![],
                    }
                };
                apply_effects(fx, &server, &dev, &mut local_sink, &engine, &backend, &pick_tx, &side_tx).await;
            }
            _ = shutdown_signal() => {
                tracing::info!("shutting down");
                engine.lock().await.save();
                return Ok(());
            }
        }
    }
}

async fn apply_effects(
    fx: Vec<Effect>,
    server: &Arc<ControlServer>,
    dev: &DeviceHandle,
    local_sink: &mut LocalSink,
    engine: &Arc<Mutex<Engine>>,
    backend: &SharedBackend,
    pick_tx: &mpsc::UnboundedSender<PickOutcome>,
    side_tx: &mpsc::UnboundedSender<SideOutcome>,
) {
    for f in fx {
        match f {
            Effect::Broadcast(ev) => server.broadcast(Message::Event(ev)),
            Effect::Recenter => {
                let _ = dev.cmd_tx.send(DeviceCommand::Recenter);
            }
            Effect::Calibrate { seconds } => {
                let _ = dev.cmd_tx.send(DeviceCommand::Calibrate(seconds));
            }
            Effect::SetDisplayMode(m) => {
                let _ = dev.cmd_tx.send(DeviceCommand::SetDisplayMode(m));
            }
            Effect::SetBrightness(b) => {
                let _ = dev.cmd_tx.send(DeviceCommand::SetBrightness(b));
            }
            Effect::PickSources { multiple, size } => {
                if crate::capture::PERCH_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
                    tracing::warn!("send window: Perch's own window is focused; focus the app first");
                    server.broadcast(Message::Event(Event::Error {
                        message: "focus the app you want on the glasses, then press Ctrl+Alt+G".into(),
                    }));
                } else {
                    spawn_pick(Arc::clone(backend), None, multiple, size, pick_tx.clone());
                }
            }
            Effect::ReleaseSource(source) => {
                if let Ok(mut b) = backend.lock() {
                    if let Some(b) = b.as_mut() {
                        b.release(&source);
                    }
                }
            }
            Effect::CloseApp { id, source } => {
                let backend = Arc::clone(backend);
                let tx = side_tx.clone();
                tokio::task::spawn_blocking(move || {
                    if let Ok(mut b) = backend.lock() {
                        if let Some(b) = b.as_mut() {
                            b.close_app(&source);
                        }
                    }
                    let _ = tx.send(SideOutcome::Closed(id));
                });
            }
            Effect::ResizeDisplay { id, source, size } => {
                let backend = Arc::clone(backend);
                let tx = side_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let res = (|| {
                        let mut guard = backend.lock().map_err(|_| "capture backend poisoned".to_string())?;
                        let b = guard.as_mut().ok_or_else(|| "no capture backend".to_string())?;
                        let new = b.resize_display(&source, size.0, size.1).map_err(|e| e.to_string())?;
                        let px = b.source_size(&new).unwrap_or(size);
                        Ok((new, px))
                    })();
                    let _ = tx.send(SideOutcome::Resized(id, res));
                });
            }
            Effect::DeliverInput { peer, events } => {
                let me = engine.lock().await.me.peer_id;
                if peer == me {
                    if let Some(sink) = local_sink.as_mut() {
                        for e in events {
                            if let Err(err) = sink.inject(e) {
                                tracing::debug!("inject: {err}");
                            }
                        }
                        let _ = sink.flush();
                    }
                } else {
                    server.send_to(peer, Message::Input { events }).await;
                }
            }
        }
    }
}

/// Ctrl-C or SIGTERM (what `pkill`/systemd send), so shutdown is always clean.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(t) => t,
            Err(_) => return tokio::signal::ctrl_c().await.unwrap_or(()),
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
