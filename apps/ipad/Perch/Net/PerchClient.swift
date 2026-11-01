import Foundation
import Network
import Combine

/// Controller client for `perchd`: Bonjour discovery, JSON control over
/// WebSocket, batched input over UDP. Mirrors the Android `PerchClient`.
@MainActor
final class PerchClient: ObservableObject {
    struct Host: Identifiable, Hashable { var id: String { name }; let name: String; let endpoint: NWEndpoint }
    struct WindowInfo: Identifiable, Hashable { let id: String; let label: String; let focused: Bool; let visible: Bool }

    @Published var hosts: [Host] = []
    @Published var connected = false
    @Published var hostName = ""
    @Published var glasses: String?
    @Published var calibration = "uncalibrated"
    @Published var windows: [WindowInfo] = []
    @Published var layouts: [String] = []
    @Published var lastError: String?

    private let peerId = UUID().uuidString.lowercased()
    private let deviceName: String
    private var ws: URLSessionWebSocketTask?
    private var udp: NWConnection?
    private var browser: NWBrowser?
    private var session = URLSession(configuration: .default)

    init(deviceName: String) { self.deviceName = deviceName }

    // MARK: discovery
    func startDiscovery() {
        let b = NWBrowser(for: .bonjour(type: "_perch._tcp", domain: nil), using: .tcp)
        b.browseResultsChangedHandler = { [weak self] results, _ in
            Task { @MainActor in
                self?.hosts = results.compactMap { r in
                    if case let .service(name, _, _, _) = r.endpoint { return Host(name: name, endpoint: r.endpoint) }
                    return nil
                }
            }
        }
        b.start(queue: .main)
        browser = b
    }
    func stopDiscovery() { browser?.cancel(); browser = nil }

    // MARK: connection
    func connect(host: String, port: Int = 7420, token: String = "") {
        disconnect()
        guard let url = URL(string: "ws://\(host):\(port)") else { return }
        let task = session.webSocketTask(with: url)
        ws = task
        task.resume()
        let name = token.isEmpty ? deviceName : "\(deviceName)#\(token)"
        send(json: [
            "msg": "hello",
            "version": ["major": 1, "minor": 0],
            "peer_id": peerId, "name": name, "platform": "i_pad_os",
            "caps": ["glasses_host": false, "screen_source": false, "input_sink": false, "input_source": true, "dex": false],
        ])
        send(json: ["msg": "command", "cmd": "get_state"])
        receiveLoop()
        let u = NWConnection(host: NWEndpoint.Host(host), port: NWEndpoint.Port(integerLiteral: 7421), using: .udp)
        u.start(queue: .global(qos: .userInteractive))
        udp = u
        sendUdp(Postcard.ping(1))
    }

    /// Resolve a Bonjour result then connect.
    func connect(to h: Host) {
        let c = NWConnection(to: h.endpoint, using: .tcp)
        c.stateUpdateHandler = { [weak self] st in
            if case .ready = st, let path = c.currentPath, case let .hostPort(host, port)? = path.remoteEndpoint {
                Task { @MainActor in self?.connect(host: "\(host)".replacingOccurrences(of: "%.*", with: "", options: .regularExpression), port: Int(port.rawValue)) }
                c.cancel()
            }
        }
        c.start(queue: .main)
    }

    func disconnect() { ws?.cancel(with: .goingAway, reason: nil); ws = nil; udp?.cancel(); udp = nil; connected = false }

    private func receiveLoop() {
        ws?.receive { [weak self] result in
            guard let self else { return }
            switch result {
            case let .success(.string(text)): Task { @MainActor in self.handle(text); self.receiveLoop() }
            case .success: Task { @MainActor in self.receiveLoop() }
            case let .failure(e): Task { @MainActor in self.connected = false; self.lastError = e.localizedDescription }
            }
        }
    }

    private func handle(_ text: String) {
        guard let data = text.data(using: .utf8), let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        switch o["msg"] as? String {
        case "hello": connected = true; hostName = o["name"] as? String ?? ""
        case "event":
            switch o["event"] as? String {
            case "state": applyState(o)
            case "glasses_connected": glasses = o["model"] as? String
            case "glasses_disconnected": glasses = nil
            case "focus_changed": let id = o["id"] as? String; windows = windows.map { WindowInfo(id: $0.id, label: $0.label, focused: $0.id == id, visible: $0.visible) }
            case "window_changed", "window_removed": send(json: ["msg": "command", "cmd": "get_state"])
            case "error": lastError = o["message"] as? String
            default: break
            }
        default: break
        }
    }

    private func applyState(_ o: [String: Any]) {
        glasses = o["glasses"] as? String
        layouts = o["layouts"] as? [String] ?? []
        if let c = o["calibration"] { calibration = "\(c)" }
        let focused = o["focused"] as? String
        windows = (o["windows"] as? [[String: Any]] ?? []).map { w in
            let src = w["source"] as? [String: Any] ?? [:]
            let label: String
            switch src["kind"] as? String {
            case "display": label = "Display \(src["display_index"] ?? 0)"
            case "window": label = src["title"] as? String ?? "Window"
            case "virtual": label = "Virtual"
            case "dex": label = "DeX"
            case "phone_screen": label = "Phone"
            default: label = "?"
            }
            let id = w["id"] as? String ?? ""
            return WindowInfo(id: id, label: label, focused: id == focused, visible: w["visible"] as? Bool ?? true)
        }
    }

    // MARK: commands
    private func send(json: [String: Any]) {
        guard let d = try? JSONSerialization.data(withJSONObject: json), let s = String(data: d, encoding: .utf8) else { return }
        ws?.send(.string(s)) { _ in }
    }
    func recenter() { send(json: ["msg": "command", "cmd": "recenter"]) }
    func calibrate(seconds: Int = 20) { send(json: ["msg": "command", "cmd": "calibrate", "seconds": seconds]) }
    func focusNext() { send(json: ["msg": "command", "cmd": "focus_next"]) }
    func focus(_ id: String) { send(json: ["msg": "command", "cmd": "focus", "id": id]) }
    func pinHere(_ id: String) { send(json: ["msg": "command", "cmd": "pin_here", "id": id]) }
    func preset(_ name: String) { send(json: ["msg": "command", "cmd": "apply_preset", "name": name]) }
    func setMode(_ mode: String) { send(json: ["msg": "command", "cmd": "set_display_mode", "mode": mode]) }

    // MARK: input
    func sendInput(_ events: [Postcard.Event]) { sendUdp(Postcard.input(events)) }
    private func sendUdp(_ d: Data) { udp?.send(content: d, completion: .contentProcessed { _ in }) }
}
