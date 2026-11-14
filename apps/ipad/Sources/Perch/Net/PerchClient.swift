import Combine
import Foundation
import Network
import PerchCore

/// Controller client for `perchd`: Bonjour discovery, JSON control over
/// WebSocket, batched input over UDP (docs/PROTOCOL.md). Mirrors the Android
/// `PerchClient`; parsing and input encoding live in `PerchCore`.
@MainActor
final class PerchClient: ObservableObject {
    struct Host: Identifiable, Hashable {
        var id: String { name }
        let name: String
        let endpoint: NWEndpoint
    }

    @Published var hosts: [Host] = []
    @Published var connected = false
    @Published var connecting = false
    @Published var hostName = ""
    @Published var state = HostState()
    @Published var lastError: String?

    /// Touchpad, keyboard and controller input for the connected host.
    let remote: RemoteInput

    private let peerId = UUID().uuidString.lowercased()
    private let deviceName: String
    private let platform: String
    private let udpBox = UdpBox()
    private var ws: URLSessionWebSocketTask?
    private var browser: NWBrowser?
    private var resolver: NWConnection?
    private var grid: [String: Any]?
    private var refreshPending = false
    private let session = URLSession(configuration: .default)
    private static let lastHostKey = "lastHost"

    init(deviceName: String, platform: String) {
        self.deviceName = deviceName
        self.platform = platform
        let box = udpBox
        remote = RemoteInput { box.send(Postcard.input($0)) }
        remote.onFocusNext = { [weak self] in self?.focusNext() }
    }

    /// The address of the last host that answered, for the connect screen.
    var lastHost: String { UserDefaults.standard.string(forKey: Self.lastHostKey) ?? "" }

    // MARK: discovery
    func startDiscovery() {
        guard browser == nil else { return }
        let b = NWBrowser(for: .bonjour(type: "_perch._tcp", domain: nil), using: .tcp)
        b.browseResultsChangedHandler = { [weak self] results, _ in
            let found = results.compactMap { r -> Host? in
                if case let .service(name, _, _, _) = r.endpoint { return Host(name: name, endpoint: r.endpoint) }
                return nil
            }.sorted { $0.name < $1.name }
            Task { @MainActor in self?.hosts = found }
        }
        b.start(queue: .main)
        browser = b
    }

    func stopDiscovery() { browser?.cancel(); browser = nil }

    // MARK: connection
    func connect(host: String, port: Int = 7420, token: String = "") {
        disconnect()
        // IPv6 literals need brackets in a URL.
        let h = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
        guard !host.isEmpty, let url = URL(string: "ws://\(h):\(port)") else {
            lastError = "\"\(host)\" is not a host name or address"
            return
        }
        lastError = nil
        connecting = true
        let task = session.webSocketTask(with: url)
        ws = task
        task.resume()
        let name = token.isEmpty ? deviceName : "\(deviceName)#\(token)"
        send(json: [
            "msg": "hello",
            "version": ["major": 1, "minor": 6],
            "peer_id": peerId, "name": name, "platform": platform,
            "caps": ["glasses_host": false, "screen_source": false, "input_sink": false, "input_source": true, "dex": false],
        ])
        send(json: ["msg": "command", "cmd": "get_state"])
        receiveLoop(task, host: host)
        udpBox.open(host: host, port: 7421)
        udpBox.send(Postcard.ping(1))
    }

    /// Resolve a Bonjour result to an IPv4 address, then connect.
    func connect(to h: Host) {
        resolver?.cancel()
        lastError = nil
        connecting = true
        let params = NWParameters.tcp
        (params.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options)?.version = .v4
        let c = NWConnection(to: h.endpoint, using: params)
        resolver = c
        c.stateUpdateHandler = { [weak self] st in
            switch st {
            case .ready:
                if case let .hostPort(host, port)? = c.currentPath?.remoteEndpoint {
                    // Drop the interface scope ("192.168.1.5%en0").
                    let addr = "\(host)".replacingOccurrences(of: "%.*", with: "", options: .regularExpression)
                    Task { @MainActor in self?.connect(host: addr, port: Int(port.rawValue)) }
                }
                c.cancel()
            case let .failed(e), let .waiting(e):
                Task { @MainActor in self?.connecting = false; self?.lastError = "\(h.name): \(e.localizedDescription)" }
                c.cancel()
            default: break
            }
        }
        c.start(queue: .main)
    }

    func disconnect() {
        ws?.cancel(with: .goingAway, reason: nil)
        ws = nil
        udpBox.close()
        connected = false
        connecting = false
        state = HostState()
        grid = nil
    }

    private func receiveLoop(_ task: URLSessionWebSocketTask, host: String) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self, self.ws === task else { return }   // a newer connection took over
                switch result {
                case let .success(.string(text)):
                    self.handle(text, host: host)
                    self.receiveLoop(task, host: host)
                case .success:
                    self.receiveLoop(task, host: host)
                case let .failure(e):
                    self.disconnect()
                    self.lastError = e.localizedDescription
                }
            }
        }
    }

    private func handle(_ text: String, host: String) {
        guard let data = text.data(using: .utf8),
              let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        switch o["msg"] as? String {
        case "hello":
            connected = true
            connecting = false
            hostName = o["name"] as? String ?? host
            UserDefaults.standard.set(host, forKey: Self.lastHostKey)
        case "event":
            switch o["event"] as? String {
            case "state":
                let parsed = HostState.parse(o)
                state = parsed.state
                grid = parsed.grid
            case "glasses_connected": state.glasses = o["model"] as? String
            case "glasses_disconnected": state.glasses = nil
            case "focus_changed":
                let id = o["id"] as? String
                for i in state.windows.indices { state.windows[i].focused = state.windows[i].id == id }
            case "calibration":
                // Arrives once per percent while calibrating: read it in place.
                if let c = Calibration(event: o) { state.calibration = c } else { refreshSoon() }
            case "window_changed", "window_removed": refreshSoon()
            case "error": lastError = o["message"] as? String
            default: break   // pose: the panel does not draw it
            }
        default: break
        }
    }

    /// One `get_state` for a burst of change events (a re-flow sends one per window).
    private func refreshSoon() {
        guard !refreshPending else { return }
        refreshPending = true
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 60_000_000)
            self?.refreshPending = false
            self?.cmd("get_state")
        }
    }

    // MARK: commands
    private func send(json: [String: Any]) {
        guard let d = try? JSONSerialization.data(withJSONObject: json), let s = String(data: d, encoding: .utf8) else { return }
        ws?.send(.string(s)) { _ in }
    }

    private func cmd(_ name: String, _ fields: [String: Any] = [:]) {
        var o = fields
        o["msg"] = "command"
        o["cmd"] = name
        send(json: o)
    }

    func recenter() { cmd("recenter") }
    func calibrate(seconds: Int = 20) { cmd("calibrate", ["seconds": seconds]) }
    func focusNext() { cmd("focus_next") }
    func focus(_ id: String) { cmd("focus", ["id": id]) }
    func pinHere(_ id: String) { cmd("pin_here", ["id": id]) }
    func pinFocused() { if let f = state.windows.first(where: \.focused) { pinHere(f.id) } }
    func preset(_ name: String) { cmd("apply_preset", ["name": name]) }
    func loadLayout(_ name: String) { cmd("load_layout", ["name": name]) }
    func arrange() { cmd("arrange_grid") }
    func setMode(_ mode: String) { cmd("set_display_mode", ["mode": mode]); cmd("get_state") }
    func setBrightness(_ percent: Int) { cmd("set_brightness", ["percent": min(max(percent, 0), 100)]) }
    /// Move the host's focused window onto a new glasses display of that size.
    func moveToGlasses(width: Int, height: Int) { cmd("move_to_glasses", ["width": width, "height": height]) }
    /// Take the window off the glasses (the host moves it back to its own screen).
    func remove(_ id: String) { cmd("remove_window", ["id": id]) }
    func removeAll() { state.windows.forEach { remove($0.id) } }
    func closeApp(_ id: String) { cmd("close_app_window", ["id": id]) }
    /// Width in layout blocks; 0 = automatic from the window's pixels.
    func setSpan(_ id: String, _ span: Int) { cmd("set_window_span", ["id": id, "span": span]) }
    /// Move the whole layout nearer (negative) or further away, metres.
    func moveLayout(by delta: Double) {
        guard let g = HostState.grid(grid, distanceBy: delta) else { return }
        cmd("set_grid", ["grid": g])
    }
}

/// The UDP side, usable from any thread: `NWConnection` is thread-safe and the
/// input path must not wait for the main actor.
private final class UdpBox: @unchecked Sendable {
    private let lock = NSLock()
    private var conn: NWConnection?

    func open(host: String, port: UInt16) {
        let c = NWConnection(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: port)!, using: .udp)
        c.start(queue: .global(qos: .userInteractive))
        lock.lock(); conn?.cancel(); conn = c; lock.unlock()
    }

    func close() { lock.lock(); conn?.cancel(); conn = nil; lock.unlock() }

    func send(_ d: Data) {
        lock.lock(); let c = conn; lock.unlock()
        c?.send(content: d, completion: .contentProcessed { _ in })
    }
}
