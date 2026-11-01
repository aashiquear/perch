//! `perch-net` — how Perch peers find and talk to each other.
//!
//! Three pieces:
//!
//! 1. [`Discovery`] — advertise/browse `_perch._tcp.local.` with mDNS so the
//!    phone finds the laptop (and vice versa) with zero config.
//! 2. [`ControlServer`] / [`ControlClient`] — a WebSocket carrying JSON
//!    [`Message`]s: handshake, commands, events, state. Low rate, human
//!    readable (`websocat ws://laptop:7420`).
//! 3. [`StreamSocket`] — a UDP socket carrying postcard [`StreamPacket`]s:
//!    poses at IMU rate and batched input events. Loss-tolerant by design;
//!    a lost pose is superseded 1 ms later.
//!
//! Security: v0.1 is LAN-only and unauthenticated beyond a shared pairing
//! token checked at `Hello` time (see [`ControlServer::with_token`]). TLS /
//! Noise is on the roadmap (docs/ROADMAP.md §Security). Do not expose 7420/7421
//! beyond your LAN.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use futures_util::{SinkExt, StreamExt};
use perch_proto::{
    Hello, Message, StreamPacket, DEFAULT_CONTROL_PORT, DEFAULT_STREAM_PORT, MDNS_SERVICE, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

/// Errors.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// I/O.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// WebSocket.
    #[error("websocket: {0}")]
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    /// Protocol.
    #[error("protocol: {0}")]
    Proto(#[from] perch_proto::ProtoError),
    /// mDNS.
    #[error("mdns: {0}")]
    Mdns(String),
    /// Handshake failed.
    #[error("handshake: {0}")]
    Handshake(String),
}

impl From<tokio_tungstenite::tungstenite::Error> for NetError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        NetError::Ws(Box::new(e))
    }
}

/// Result alias.
pub type Result<T> = std::result::Result<T, NetError>;

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// A peer seen on the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// Instance name (the peer's human name).
    pub name: String,
    /// Addresses to try.
    pub addrs: Vec<SocketAddr>,
    /// Peer id if advertised in TXT.
    pub peer_id: Option<Uuid>,
}

/// mDNS advertise + browse.
pub struct Discovery {
    daemon: mdns_sd::ServiceDaemon,
}

impl Discovery {
    /// Start the mDNS daemon.
    pub fn new() -> Result<Self> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| NetError::Mdns(e.to_string()))?;
        Ok(Self { daemon })
    }

    /// Advertise this host on `port`.
    pub fn advertise(&self, name: &str, peer_id: Uuid, port: u16) -> Result<()> {
        let host = format!("{}.local.", hostname());
        let props: HashMap<String, String> = [
            ("peer_id".to_string(), peer_id.to_string()),
            ("proto".to_string(), PROTOCOL_VERSION.to_string()),
        ]
        .into_iter()
        .collect();
        let info = mdns_sd::ServiceInfo::new(MDNS_SERVICE, name, &host, "", port, props)
            .map_err(|e| NetError::Mdns(e.to_string()))?
            .enable_addr_auto();
        self.daemon.register(info).map_err(|e| NetError::Mdns(e.to_string()))?;
        tracing::info!(%name, port, "advertising on mDNS");
        Ok(())
    }

    /// Browse for peers; sends each resolved peer on the returned channel.
    pub fn browse(&self) -> Result<mpsc::UnboundedReceiver<DiscoveredPeer>> {
        let rx = self
            .daemon
            .browse(MDNS_SERVICE)
            .map_err(|e| NetError::Mdns(e.to_string()))?;
        let (tx, out) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                if let mdns_sd::ServiceEvent::ServiceResolved(info) = ev {
                    let port = info.get_port();
                    let peer = DiscoveredPeer {
                        name: info.get_fullname().split('.').next().unwrap_or("").to_string(),
                        addrs: info
                            .get_addresses()
                            .iter()
                            .map(|ip| SocketAddr::new(*ip, port))
                            .collect(),
                        peer_id: info.get_property_val_str("peer_id").and_then(|s| s.parse().ok()),
                    };
                    if tx.send(peer).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(out)
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "perch".into())
}

// ---------------------------------------------------------------------------
// Control channel (server)
// ---------------------------------------------------------------------------

/// An inbound message tagged with who sent it.
#[derive(Debug, Clone)]
pub struct Inbound {
    /// Sender's peer id (from its `Hello`).
    pub from: Uuid,
    /// The message.
    pub msg: Message,
}

/// WebSocket control server. Clients connect, send `Hello`, then exchange
/// `Command`/`Event`/`Input`.
pub struct ControlServer {
    hello: Hello,
    token: Option<String>,
    peers: Arc<RwLock<HashMap<Uuid, Hello>>>,
    inbound_tx: mpsc::Sender<Inbound>,
    outbound: broadcast::Sender<Message>,
    direct: Arc<RwLock<HashMap<Uuid, mpsc::UnboundedSender<Message>>>>,
}

impl ControlServer {
    /// Create a server that identifies itself with `hello`.
    pub fn new(hello: Hello) -> (Self, mpsc::Receiver<Inbound>) {
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);
        let (outbound, _) = broadcast::channel(256);
        (
            Self {
                hello,
                token: None,
                peers: Default::default(),
                inbound_tx,
                outbound,
                direct: Default::default(),
            },
            inbound_rx,
        )
    }

    /// Require clients to present this pairing token in `Hello.name` suffix
    /// `#token` (simple v0.1 scheme; see crate docs).
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Broadcast to every connected client.
    pub fn broadcast(&self, msg: Message) {
        let _ = self.outbound.send(msg);
    }

    /// Send to one client.
    pub async fn send_to(&self, peer: Uuid, msg: Message) {
        if let Some(tx) = self.direct.read().await.get(&peer) {
            let _ = tx.send(msg);
        }
    }

    /// Currently connected peers.
    pub async fn peers(&self) -> Vec<Hello> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Bind and serve forever.
    pub async fn serve(self: Arc<Self>, bind: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(bind).await?;
        tracing::info!(%bind, "control server listening");
        loop {
            let (stream, addr) = listener.accept().await?;
            let me = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = me.handle(stream, addr).await {
                    tracing::warn!(%addr, "control connection ended: {e}");
                }
            });
        }
    }

    async fn handle(self: Arc<Self>, stream: TcpStream, addr: SocketAddr) -> Result<()> {
        let ws = tokio_tungstenite::accept_async(stream).await?;
        let (mut sink, mut source) = ws.split();

        // Handshake: first frame must be Hello.
        let first = source
            .next()
            .await
            .ok_or_else(|| NetError::Handshake("closed".into()))??;
        let hello = match first {
            WsMessage::Text(t) => match Message::from_json(&t)? {
                Message::Hello(h) => h,
                _ => return Err(NetError::Handshake("first message must be hello".into())),
            },
            _ => return Err(NetError::Handshake("expected text frame".into())),
        };
        if !PROTOCOL_VERSION.compatible(&hello.version) {
            return Err(NetError::Handshake(format!("incompatible version {}", hello.version)));
        }
        if let Some(tok) = &self.token {
            if !hello.name.ends_with(&format!("#{tok}")) {
                return Err(NetError::Handshake("bad pairing token".into()));
            }
        }
        let peer_id = hello.peer_id;
        tracing::info!(%addr, %peer_id, name = %hello.name, "peer connected");
        sink.send(WsMessage::Text(Message::Hello(self.hello.clone()).to_json()?.into()))
            .await?;

        let (direct_tx, mut direct_rx) = mpsc::unbounded_channel::<Message>();
        self.peers.write().await.insert(peer_id, hello.clone());
        self.direct.write().await.insert(peer_id, direct_tx);
        let mut bcast = self.outbound.subscribe();

        let inbound = self.inbound_tx.clone();
        let reader = async {
            while let Some(frame) = source.next().await {
                match frame? {
                    WsMessage::Text(t) => {
                        let msg = Message::from_json(&t)?;
                        if inbound.send(Inbound { from: peer_id, msg }).await.is_err() {
                            break;
                        }
                    }
                    WsMessage::Close(_) => break,
                    _ => {}
                }
            }
            Ok::<(), NetError>(())
        };
        let writer = async {
            loop {
                let msg = tokio::select! {
                    m = direct_rx.recv() => match m { Some(m) => m, None => break },
                    m = bcast.recv() => match m {
                        Ok(m) => m,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(%peer_id, "client lagged {n} messages");
                            continue;
                        }
                        Err(_) => break,
                    },
                };
                sink.send(WsMessage::Text(msg.to_json()?.into())).await?;
            }
            Ok::<(), NetError>(())
        };
        let res = tokio::select! {
            r = reader => r,
            r = writer => r,
        };
        self.peers.write().await.remove(&peer_id);
        self.direct.write().await.remove(&peer_id);
        tracing::info!(%peer_id, "peer disconnected");
        res
    }
}

// ---------------------------------------------------------------------------
// Control channel (client)
// ---------------------------------------------------------------------------

/// A client connection to a [`ControlServer`].
pub struct ControlClient {
    /// The server's hello.
    pub server: Hello,
    tx: mpsc::UnboundedSender<Message>,
    rx: mpsc::UnboundedReceiver<Message>,
    io: tokio::task::JoinHandle<()>,
}

impl ControlClient {
    /// Connect to `ws://host:port`, perform the handshake.
    pub async fn connect(url: &str, hello: Hello) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url).await?;
        let (mut sink, mut source) = ws.split();
        sink.send(WsMessage::Text(Message::Hello(hello).to_json()?.into()))
            .await?;
        let first = source
            .next()
            .await
            .ok_or_else(|| NetError::Handshake("closed".into()))??;
        let server = match first {
            WsMessage::Text(t) => match Message::from_json(&t)? {
                Message::Hello(h) => h,
                _ => return Err(NetError::Handshake("expected hello".into())),
            },
            _ => return Err(NetError::Handshake("expected text".into())),
        };
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<Message>();
        let io = tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = out_rx.recv() => match m {
                        Some(m) => {
                            if let Ok(j) = m.to_json() {
                                if sink.send(WsMessage::Text(j.into())).await.is_err() { break; }
                            }
                        }
                        None => {
                            // Client dropped: close politely so the server doesn't log a reset.
                            let _ = sink.send(WsMessage::Close(None)).await;
                            break;
                        }
                    },
                    f = source.next() => match f {
                        Some(Ok(WsMessage::Text(t))) => {
                            if let Ok(m) = Message::from_json(&t) {
                                if in_tx.send(m).is_err() { break; }
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
        });
        Ok(Self {
            server,
            tx: out_tx,
            rx: in_rx,
            io,
        })
    }

    /// Send a message.
    pub fn send(&self, msg: Message) -> bool {
        self.tx.send(msg).is_ok()
    }

    /// Receive the next message (None when disconnected).
    pub async fn recv(&mut self) -> Option<Message> {
        self.rx.recv().await
    }

    /// Close politely: sends a WebSocket Close frame and waits (≤ 500 ms) for
    /// the I/O task to finish, so short-lived clients like the CLI don't leave
    /// "connection reset" noise in the server log.
    pub async fn close(self) {
        drop(self.tx);
        let _ = tokio::time::timeout(std::time::Duration::from_millis(500), self.io).await;
    }
}

// ---------------------------------------------------------------------------
// Stream channel
// ---------------------------------------------------------------------------

/// UDP socket for [`StreamPacket`]s.
pub struct StreamSocket {
    sock: UdpSocket,
}

impl StreamSocket {
    /// Bind.
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        Ok(Self {
            sock: UdpSocket::bind(addr).await?,
        })
    }

    /// Send a packet to `to`.
    pub async fn send_to(&self, pkt: &StreamPacket, to: SocketAddr) -> Result<()> {
        self.sock.send_to(&pkt.to_postcard()?, to).await?;
        Ok(())
    }

    /// Receive the next packet.
    pub async fn recv(&self) -> Result<(StreamPacket, SocketAddr)> {
        let mut buf = [0u8; 2048];
        loop {
            let (n, from) = self.sock.recv_from(&mut buf).await?;
            match StreamPacket::from_postcard(&buf[..n]) {
                Ok(p) => return Ok((p, from)),
                Err(e) => tracing::debug!(%from, "bad stream packet: {e}"),
            }
        }
    }

    /// Local address.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.sock.local_addr()?)
    }
}

/// Default bind address for the control server.
pub fn default_control_bind() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], DEFAULT_CONTROL_PORT))
}
/// Default bind address for the stream socket.
pub fn default_stream_bind() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], DEFAULT_STREAM_PORT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_proto::{Capabilities, Command, Event, Platform};

    fn hello(name: &str) -> Hello {
        Hello {
            version: PROTOCOL_VERSION,
            peer_id: Uuid::new_v4(),
            name: name.into(),
            platform: Platform::Linux,
            caps: Capabilities::default(),
        }
    }

    #[tokio::test]
    async fn control_roundtrip() {
        let (server, mut inbound) = ControlServer::new(hello("host"));
        let server = Arc::new(server);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let s2 = Arc::clone(&server);
        tokio::spawn(async move { s2.serve(addr).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client = ControlClient::connect(&format!("ws://{addr}"), hello("phone"))
            .await
            .unwrap();
        assert_eq!(client.server.name, "host");
        assert!(client.send(Message::Command(Command::Recenter)));
        let got = inbound.recv().await.unwrap();
        assert_eq!(got.msg, Message::Command(Command::Recenter));

        server.broadcast(Message::Event(Event::GlassesDisconnected));
        let ev = client.recv().await.unwrap();
        assert_eq!(ev, Message::Event(Event::GlassesDisconnected));
        assert_eq!(server.peers().await.len(), 1);
    }

    #[tokio::test]
    async fn token_rejects_unpaired() {
        let (server, _inbound) = ControlServer::new(hello("host"));
        let server = Arc::new(server.with_token("1234"));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        tokio::spawn(async move { server.serve(addr).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(ControlClient::connect(&format!("ws://{addr}"), hello("phone"))
            .await
            .is_err());
        assert!(ControlClient::connect(&format!("ws://{addr}"), hello("phone#1234"))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn stream_roundtrip() {
        let a = StreamSocket::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let b = StreamSocket::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let pkt = StreamPacket::Ping(7);
        a.send_to(&pkt, b.local_addr().unwrap()).await.unwrap();
        let (got, _) = b.recv().await.unwrap();
        assert_eq!(got, pkt);
    }
}
