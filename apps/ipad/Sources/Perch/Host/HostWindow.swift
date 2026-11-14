import UIKit
import WebKit

/// One window on the glasses: a live web view (page, video, PDF, local file)
/// inside a container whose layer is warped every frame by the homography the
/// Rust core computes. No texture copy: Core Animation draws the real view
/// in perspective, so video keeps playing natively.
///
/// All windows share the default website data store, so a sign-in made in
/// one holds in every window and across launches.
final class HostWindow: NSObject, Identifiable {
    static let wide = CGSize(width: 1440, height: 810)
    static let tall = CGSize(width: 810, height: 1440)
    /// The glasses panel, pixel for pixel.
    static let panel = CGSize(width: 1920, height: 1080)

    let id = UUID()
    let container: UIView
    let web: WKWebView
    private let cursor = UIView()
    private let frameLine = CAShapeLayer()
    private let away = UILabel()
    private var scopedFile: URL?
    private var lastHover = 0.0
    private var observers: [NSKeyValueObservation] = []

    /// Page size in points (= glasses pixels at the source; the screen's scale is 1).
    private(set) var size: CGSize
    /// Physical width in metres at the layout distance.
    var widthM: Double
    var heightM: Double { widthM * Double(size.height / size.width) }
    /// Fixed to the head (fills the view, for games) instead of the room.
    var headLocked = false
    /// Where "pin here" put it (yaw, pitch in radians); nil = its place in the row.
    var pinned: (yaw: Float, pitch: Float)?
    /// The page is being used on the iPad's own screen (sign-in).
    private(set) var onIPad = false
    /// Title or address changed.
    var onChange: () -> Void = {}
    /// A diagnostic line from the page (taps, fullscreen, script errors).
    var onLog: (String) -> Void = { _ in }

    private(set) var cursorAt: CGPoint
    var title: String { web.title.flatMap { $0.isEmpty ? nil : $0 } ?? web.url?.host ?? web.url?.lastPathComponent ?? "Window" }
    var subtitle: String { web.url.map { $0.isFileURL ? $0.lastPathComponent : ($0.host ?? $0.absoluteString) } ?? "" }
    var address: String { web.url.map { $0.isFileURL ? $0.lastPathComponent : $0.absoluteString } ?? "" }

    init(size: CGSize, widthM: Double) {
        self.size = size
        self.widthM = widthM
        cursorAt = CGPoint(x: size.width / 2, y: size.height / 2)
        let cfg = WKWebViewConfiguration()
        cfg.allowsInlineMediaPlayback = true
        // Clicks come from the touchpad as script, so playback must not wait for a "real" touch.
        cfg.mediaTypesRequiringUserActionForPlayback = []
        cfg.allowsPictureInPictureMediaPlayback = false
        // Present as Safari: sign-in pages and cloud-gaming sites turn away
        // "embedded browsers", which is what a bare web view announces.
        let os = ProcessInfo.processInfo.operatingSystemVersion.majorVersion
        cfg.applicationNameForUserAgent = "Version/\(os).0 Safari/605.1.15"
        // A tall window is phone-shaped: ask for the phone page.
        cfg.defaultWebpagePreferences.preferredContentMode = size.height > size.width ? .mobile : .desktop
        for script in [Script.inputShim, Script.standaloneShim, Script.fullscreenShim, Script.gamepadShim] {
            cfg.userContentController.addUserScript(WKUserScript(source: script, injectionTime: .atDocumentStart, forMainFrameOnly: false))
        }
        let log = PageLog()
        cfg.userContentController.add(log, name: "perch")
        web = WKWebView(frame: CGRect(origin: .zero, size: size), configuration: cfg)
        web.allowsBackForwardNavigationGestures = false
        web.isOpaque = true
        web.backgroundColor = .white
        container = UIView(frame: CGRect(origin: .zero, size: size))
        super.init()
        container.backgroundColor = UIColor(white: 0.08, alpha: 1)
        container.clipsToBounds = false
        container.layer.anchorPoint = .zero
        container.layer.position = .zero
        away.text = "This window is on the iPad"
        away.textColor = UIColor(white: 0.8, alpha: 1)
        away.font = .systemFont(ofSize: 44, weight: .semibold)
        away.textAlignment = .center
        container.addSubview(away)
        container.addSubview(web)
        frameLine.fillColor = nil
        frameLine.lineWidth = 4
        container.layer.addSublayer(frameLine)
        cursor.frame = CGRect(x: 0, y: 0, width: 22, height: 22)
        cursor.layer.cornerRadius = 11
        cursor.backgroundColor = UIColor(white: 0.92, alpha: 0.9)
        cursor.layer.borderColor = UIColor.black.cgColor
        cursor.layer.borderWidth = 2
        cursor.isUserInteractionEnabled = false
        container.addSubview(cursor)
        web.uiDelegate = self
        web.navigationDelegate = self
        log.onLine = { [weak self] line in self?.onLog("\(self?.subtitle ?? ""): \(line)") }
        observers = [
            web.observe(\.title) { [weak self] _, _ in self?.onChange() },
            web.observe(\.url) { [weak self] _, _ in self?.onChange() },
        ]
        layout()
        setFocused(false)
    }

    deinit { scopedFile?.stopAccessingSecurityScopedResource() }

    private func layout() {
        container.bounds = CGRect(origin: .zero, size: size)
        away.frame = container.bounds
        if !onIPad { web.frame = container.bounds }
        frameLine.path = UIBezierPath(rect: container.bounds.insetBy(dx: -2, dy: -2)).cgPath
        cursorAt.x = min(cursorAt.x, size.width - 1); cursorAt.y = min(cursorAt.y, size.height - 1)
        cursor.center = cursorAt
    }

    func setFocused(_ on: Bool) {
        // Quiet on purpose: the focused window is only a shade lighter than the others.
        frameLine.strokeColor = UIColor(white: on ? 0.38 : 0.2, alpha: 1).cgColor
        cursor.isHidden = !on
    }

    /// Change the page size and the window's physical width together.
    func resize(to newSize: CGSize, widthM newWidth: Double) {
        size = newSize
        widthM = newWidth
        layout()
    }

    /// Turn a wide page into a tall one and back; width and height swap.
    func flip() { resize(to: CGSize(width: size.height, height: size.width), widthM: heightM) }

    // MARK: content
    func load(_ url: URL) {
        if url.isFileURL {
            // A file picked from Files: keep its permission for as long as the window lives.
            scopedFile?.stopAccessingSecurityScopedResource()
            scopedFile = url.startAccessingSecurityScopedResource() ? url : nil
            web.loadFileURL(url, allowingReadAccessTo: url)
        } else {
            web.load(URLRequest(url: url))
        }
    }

    /// Stop everything, including sound, before the window goes away.
    func shutDown() {
        observers = []
        web.stopLoading()
        web.loadHTMLString("", baseURL: nil)
        web.removeFromSuperview()
        container.removeFromSuperview()
    }

    // MARK: on the iPad's own screen
    /// Hand the live page to a view on the iPad, where it takes real touches,
    /// the on-screen keyboard and password AutoFill. The glasses show a note.
    func moveToIPad(into holder: UIView) {
        onIPad = true
        web.removeFromSuperview()
        web.frame = holder.bounds
        web.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        holder.addSubview(web)
    }

    func returnToGlasses() {
        guard onIPad else { return }
        onIPad = false
        web.removeFromSuperview()
        web.autoresizingMask = []
        web.frame = container.bounds
        container.insertSubview(web, aboveSubview: away)
    }

    // MARK: input (all in page coordinates)
    func moveCursor(dx: CGFloat, dy: CGFloat, now: Double) {
        cursorAt.x = min(max(cursorAt.x + dx, 0), size.width - 1)
        cursorAt.y = min(max(cursorAt.y + dy, 0), size.height - 1)
        cursor.center = cursorAt
        if now - lastHover > 0.1 {   // hover menus and tooltips, ten times a second
            lastHover = now
            run(Script.hover, ["x": cursorAt.x, "y": cursorAt.y])
        }
    }

    /// Use genuine touches for taps where the system allows (see `TouchSynth`).
    var realTouches = true
    /// The layer transform of the last frame (set by the host's frame loop).
    var lastTransform = CATransform3DIdentity

    /// Where a page point is on the glasses, in the stage's coordinates.
    private func onStage(_ p: CGPoint) -> CGPoint {
        let t = lastTransform
        let w = p.x * t.m14 + p.y * t.m24 + t.m44
        guard abs(w) > 1e-6 else { return p }
        return CGPoint(x: (p.x * t.m11 + p.y * t.m21 + t.m41) / w, y: (p.x * t.m12 + p.y * t.m22 + t.m42) / w)
    }

    func click() {
        if realTouches, !onIPad, let window = container.window {
            let why = MainActor.assumeIsolated { TouchSynth.tap(at: onStage(cursorAt), in: window, expected: web) }
            if why == nil { onLog("real tap at \(Int(cursorAt.x)),\(Int(cursorAt.y))"); return }
            onLog("real tap not possible (\(why ?? "")); using script")
        }
        run(Script.click, ["x": cursorAt.x, "y": cursorAt.y])
    }
    func scroll(dx: CGFloat, dy: CGFloat) { run(Script.scroll, ["x": cursorAt.x, "y": cursorAt.y, "dx": dx, "dy": dy]) }
    func type(_ text: String) { run(Script.insert, ["t": text]) }
    func backspace() { run(Script.backspace, [:]) }
    func enter() { run(Script.enter, [:]) }
    func key(_ name: String, code: Int) { run(Script.key, ["k": name, "c": code]) }
    /// Leave the page's fullscreen if it is in it; tells the caller whether it was.
    func leaveFullscreen(_ done: @escaping (Bool) -> Void) {
        web.callAsyncJavaScript(Script.leaveFullscreen, arguments: [:], in: nil, in: .page) { result in
            done((try? result.get()) as? Bool ?? false)
        }
    }

    func gamepad(axes: [Float], buttons: [Float]) { run(Script.gamepad, ["a": axes, "b": buttons]) }

    private func run(_ body: String, _ args: [String: Any]) {
        web.callAsyncJavaScript(body, arguments: args, in: nil, in: .page) { _ in }
    }
}

extension HostWindow: WKUIDelegate {
    /// Links that ask for a new tab open in this window: there is no tab strip.
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction,
                 windowFeatures: WKWindowFeatures) -> WKWebView? {
        if navigationAction.targetFrame == nil, let url = navigationAction.request.url { webView.load(URLRequest(url: url)) }
        return nil
    }

    /// Cloud gaming and calls ask for the microphone; iPadOS asks the user for the app.
    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin, initiatedByFrame frame: WKFrameInfo,
                 type: WKMediaCaptureType, decisionHandler: @escaping (WKPermissionDecision) -> Void) {
        decisionHandler(.prompt)
    }
}

extension HostWindow: WKNavigationDelegate {
    /// Stay in the window. A site with an installed app (a "universal link")
    /// would otherwise hand the address to that app: Perch drops to the
    /// background and the glasses fall back to mirroring the iPad. WebKit's
    /// "allow, but do not try the app" policy is the allow value + 2; it is
    /// not in the public header, so fall back to plain allow if it is refused.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let scheme = action.request.url?.scheme?.lowercased() ?? ""
        guard ["http", "https", "about", "blob", "data", "file", ""].contains(scheme) else {
            // An app's own scheme (store links, "open in app" buttons): not a page.
            onLog("blocked a jump to another app: \(scheme)://")
            decisionHandler(.cancel)
            return
        }
        decisionHandler(WKNavigationActionPolicy(rawValue: WKNavigationActionPolicy.allow.rawValue + 2) ?? .allow)
    }
}

/// Receives the page's log lines. The content controller retains its
/// handler, so this is a separate object and not the window itself.
private final class PageLog: NSObject, WKScriptMessageHandler {
    var onLine: (String) -> Void = { _ in }
    func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
        if let s = message.body as? String { onLine(s) }
    }
}
