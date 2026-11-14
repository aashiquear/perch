import CoreMotion
import Network
import SwiftUI
import UIKit

/// Host-mode groundwork: what this iPad can actually do towards driving the
/// glasses itself. Three things have to exist for a host: a display to draw
/// on (the glasses over USB-C), a head-motion source (the glasses' IMU, or
/// head-tracking earphones), and window content. This object checks the
/// first two on the device and keeps the findings as a report.
@MainActor
final class HostLab: ObservableObject {
    static let shared = HostLab()

    @Published var displayLines: [String] = []
    @Published var externalConnected = false
    @Published var usbLines: [String] = ["not checked yet"]
    @Published var usbUsable = false
    @Published var headLines: [String] = ["not checked yet"]
    @Published var headHz = 0.0
    @Published var headYawDeg = 0.0
    @Published var headPitchDeg = 0.0
    @Published var headRunning = false
    @Published var sendStatus = ""

    private let headphones = CMHeadphoneMotionManager()
    /// The Bluetooth check for a Bose AR headset.
    let bose = BoseProbe()
    private var externalWindow: UIWindow?

    private init() {
        // SwiftUI's life cycle did not hand the external scene to a scene delegate
        // (measured 2026-11-14), so watch for it and put our own window on it.
        let nc = NotificationCenter.default
        nc.addObserver(forName: UIScene.willConnectNotification, object: nil, queue: .main) { [weak self] n in
            let scene = n.object as? UIScene
            MainActor.assumeIsolated { self?.attach(scene) }
        }
        nc.addObserver(forName: UIScene.didDisconnectNotification, object: nil, queue: .main) { [weak self] n in
            let scene = n.object as? UIScene
            MainActor.assumeIsolated {
                guard let self, let scene, self.externalWindow?.windowScene === scene else { return }
                self.externalWindow = nil
                self.externalConnected = false
                GlassesHost.shared.screenDetached()
                self.note("external display disconnected")
            }
        }
    }

    /// Put the test card on a scene if it is the external display's.
    func attach(_ scene: UIScene?) {
        guard let ws = scene as? UIWindowScene, ws.session.role == .windowExternalDisplayNonInteractive,
              externalWindow?.windowScene !== ws else { return }
        let w = UIWindow(windowScene: ws)
        w.rootViewController = GlassesHost.shared.stage
        w.isHidden = false
        GlassesHost.shared.screenAttached(ws.screen)
        externalWindow = w
        externalConnected = true
        note("drawing on external display: \(Self.describe(ws.screen))")
        let modes = ws.screen.availableModes.map { "\(Int($0.size.width))×\(Int($0.size.height))" }
        note("modes offered: \(modes.isEmpty ? "none listed" : modes.joined(separator: ", "))")
    }
    private var samples = 0
    private var windowStart = 0.0

    // MARK: display
    func note(_ line: String) {
        let t = Date().formatted(date: .omitted, time: .standard)
        displayLines.append("\(t)  \(line)")
        if displayLines.count > 40 { displayLines.removeFirst(displayLines.count - 40) }
    }

    /// Every scene the app has and the screen it is on.
    func snapshotScenes() {
        for scene in UIApplication.shared.connectedScenes {
            attach(scene)
            guard let ws = scene as? UIWindowScene else { continue }
            let s = ws.screen
            note("scene \(scene.session.role.rawValue.replacingOccurrences(of: "UIWindowSceneSessionRole", with: "")) on \(Self.describe(s))")
        }
    }

    static func describe(_ s: UIScreen) -> String {
        let n = s.nativeBounds.size
        let main = s == UIScreen.main ? "built-in" : "EXTERNAL"
        return "\(main) \(Int(n.width))×\(Int(n.height)) px, scale \(s.scale), up to \(s.maximumFramesPerSecond) Hz"
    }

    // MARK: usb
    func checkUsb() {
        let r = UsbProbe.run()
        usbLines = r.lines
        usbUsable = r.usable
    }

    // MARK: head tracking from earphones
    func startHead() {
        headLines = [
            "earphone motion available on this iPad: \(headphones.isDeviceMotionAvailable)",
            "permission: \(Self.auth(CMHeadphoneMotionManager.authorizationStatus()))",
        ]
        guard headphones.isDeviceMotionAvailable else { return }
        samples = 0; windowStart = CACurrentMediaTime(); headRunning = true
        headphones.startDeviceMotionUpdates(to: .main) { [weak self] motion, error in
            MainActor.assumeIsolated {
                guard let self else { return }
                if let error { self.headLines.append("error: \(error.localizedDescription)"); return }
                guard let m = motion else { return }
                self.samples += 1
                let now = CACurrentMediaTime()
                if now - self.windowStart >= 1 {
                    self.headHz = Double(self.samples) / (now - self.windowStart)
                    self.samples = 0; self.windowStart = now
                }
                self.headYawDeg = m.attitude.yaw * 180 / .pi
                self.headPitchDeg = m.attitude.pitch * 180 / .pi
            }
        }
    }

    func stopHead() { headphones.stopDeviceMotionUpdates(); headRunning = false }

    private static func auth(_ s: CMAuthorizationStatus) -> String {
        switch s {
        case .authorized: return "allowed"
        case .denied: return "denied (Settings → Privacy → Motion & Fitness)"
        case .restricted: return "restricted"
        default: return "not asked yet"
        }
    }

    // MARK: report
    var report: String {
        let d = UIDevice.current
        var s = "Perch host-mode check — \(d.model), \(d.systemName) \(d.systemVersion)\n"
        s += "\n[display]\n" + (displayLines.isEmpty ? "no scene events yet" : displayLines.joined(separator: "\n"))
        s += "\nexternal display scene attached now: \(externalConnected)\n"
        s += "\n[glasses USB sensor]\n" + usbLines.joined(separator: "\n")
        s += "\nreadable by this app: \(usbUsable)\n"
        s += "\n[earphone head tracking]\n" + headLines.joined(separator: "\n")
        if headRunning || headHz > 0 { s += String(format: "\nrate %.0f Hz, yaw %.0f°, pitch %.0f°", headHz, headYawDeg, headPitchDeg) }
        s += "\n\n[Bose headphones]\n" + bose.lines.joined(separator: "\n")
        s += String(format: "\nrate %.0f Hz", bose.hz)
        s += "\n\n[pages]\n" + (GlassesHost.shared.pageLog.isEmpty ? "no lines" : GlassesHost.shared.pageLog.joined(separator: "\n"))
        return s + "\n"
    }

    /// Send the report as plain text to a listener on the laptop.
    func send(to host: String, port: UInt16 = 7430) {
        guard !host.isEmpty, let p = NWEndpoint.Port(rawValue: port) else { sendStatus = "no laptop address"; return }
        sendStatus = "sending…"
        let c = NWConnection(host: NWEndpoint.Host(host), port: p, using: .tcp)
        let data = Data(report.utf8)
        let done: @Sendable (String) -> Void = { msg in Task { @MainActor in HostLab.shared.sendStatus = msg } }
        c.stateUpdateHandler = { st in
            switch st {
            case .ready:
                c.send(content: data, isComplete: true, completion: .contentProcessed { e in
                    done(e.map { "failed: \($0.localizedDescription)" } ?? "sent to \(host)")
                    c.cancel()
                })
            case let .failed(e), let .waiting(e):
                done("failed: \(e.localizedDescription)")
                c.cancel()
            default: break
            }
        }
        c.start(queue: .main)
    }
}

