import AVFoundation
import CPerch
import PerchCore
import QuartzCore
import SwiftUI
import UIKit

/// The iPad as host: windows pinned in space on the glasses.
///
/// A thin shell around the shared Rust core, like the Android host: the
/// headset's orientation goes into `perch_ext_tracker`, the pose into
/// `perch_space`, and the homography that comes back is put on each window's
/// layer once per display refresh. Head motion comes from Bose AR headphones
/// (iPadOS refuses the glasses' own USB sensor, see hardware-log 2026-11-14);
/// without a tracker the windows stay in front of the eyes.
@MainActor
final class GlassesHost: ObservableObject {
    static let shared = GlassesHost()

    @Published private(set) var windows: [HostWindow] = []
    @Published private(set) var focused: UUID?
    @Published private(set) var displayAttached = false
    @Published private(set) var fps = 0
    @Published private(set) var tracking = false
    @Published private(set) var bookmarks = Bookmarks()
    /// Distance of the layout from the head, metres.
    @Published private(set) var distance: Float = 2.0
    /// Software dimming of the glasses picture, 0.15…1.
    @Published var brightness = 1.0 { didSet { stage.setDim(1 - brightness); defaults.set(brightness, forKey: "host.brightness") } }

    /// Taps as genuine touches (on) or as script (off). A switch, because the
    /// genuine path leans on UIKit internals and must be easy to turn off.
    @Published var realTouches = UserDefaults.standard.object(forKey: "host.realTouches") as? Bool ?? true {
        didSet { defaults.set(realTouches, forKey: "host.realTouches"); windows.forEach { $0.realTouches = realTouches } }
    }

    /// Fed by the pop-up's touchpad, typing bar, controller skins and hardware
    /// keyboard; the events it would send to a laptop are carried out here.
    private(set) lazy var input = RemoteInput { [weak self] events in
        MainActor.assumeIsolated { self?.apply(events) }
    }

    /// Recent diagnostic lines from the pages, for the Check report.
    private(set) var pageLog: [String] = []

    let stage = StageController()
    private var space: OpaquePointer?
    private var tracker: OpaquePointer?
    private var link: CADisplayLink?
    private var controllers: ControllerBridge?
    private var lastTick = 0.0, lastSample = 0.0
    private var frames = 0, fpsStart = 0.0
    private var screenSize = CGSize(width: 1920, height: 1080)
    private var restored = false
    private let defaults = UserDefaults.standard
    private let gap: Float = 0.08
    /// Horizontal field of view of the glasses. The Rokid Max figure is not
    /// established (docs/DEVICE-ROKID-MAX.md); 46° is the project default.
    private var fovDeg: Double { defaults.object(forKey: "host.fovDeg") as? Double ?? 46 }

    private init() {
        tracker = perch_ext_tracker_new(1, 20_000)
        if let b = defaults.string(forKey: "host.bookmarks").flatMap(Bookmarks.init(json:)) { bookmarks = b }
        if let d = defaults.object(forKey: "host.distance") as? Double { distance = Float(d) }
        if let b = defaults.object(forKey: "host.brightness") as? Double { brightness = min(max(b, 0.15), 1) }
        input.onFocusNext = { [weak self] in self?.focusNext() }
        HostLab.shared.bose.onSample = { [weak self] ticks, quat in self?.headSample(ticks: ticks, quat: quat) }
        // Media volume keys and the headphones' own volume act on "playback" audio.
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try? AVAudioSession.sharedInstance().setActive(true)
        controllers = ControllerBridge { [weak self] axes, buttons in self?.gamepad(axes: axes, buttons: buttons) }
        rebuild()
    }

    var focusedWindow: HostWindow? { windows.first { $0.id == focused } }

    // MARK: display
    func screenAttached(_ screen: UIScreen) {
        screenSize = screen.bounds.size
        displayAttached = true
        link?.invalidate()
        let l = screen.displayLink(withTarget: DisplayTick(self), selector: #selector(DisplayTick.tick(_:)))
        l?.add(to: .main, forMode: .common)
        link = l
        stage.setDim(1 - brightness)
        if !restored { restored = true; restore() }
        rebuild()
    }

    func screenDetached() {
        link?.invalidate(); link = nil
        displayAttached = false; fps = 0
    }

    // MARK: tracker
    /// One orientation sample from the headset (sensor clock in ms, x y z w).
    func headSample(ticks: Int, quat: [Double]) {
        guard let tracker, quat.count == 4 else { return }
        let q = quat.map(Float.init)
        lastSample = CACurrentMediaTime()
        _ = perch_ext_tracker_feed(tracker, UInt16(truncatingIfNeeded: ticks), lastSample, q)
    }

    func recenter() { if let tracker { perch_ext_tracker_recenter(tracker) } }

    // MARK: windows
    @discardableResult
    func add(_ url: URL, tall: Bool = false) -> HostWindow {
        let w = HostWindow(size: tall ? HostWindow.tall : HostWindow.wide, widthM: tall ? 0.72 : 1.3)
        w.realTouches = realTouches
        w.onChange = { [weak self] in self?.objectWillChange.send(); self?.save() }
        w.onLog = { [weak self] line in
            guard let self else { return }
            self.pageLog.append(line)
            if self.pageLog.count > 120 { self.pageLog.removeFirst(self.pageLog.count - 120) }
        }
        w.load(url)
        windows.append(w)
        stage.view.addSubview(w.container)
        focus(w.id)
        rebuild()
        return w
    }

    /// Show an address in the focused window, or in a new one if there is none.
    func open(_ url: URL) {
        if let w = focusedWindow { w.load(url) } else { add(url) }
    }

    func close(_ id: UUID) {
        guard let i = windows.firstIndex(where: { $0.id == id }) else { return }
        windows[i].shutDown()
        windows.remove(at: i)
        if focused == id { focused = windows.first?.id }
        refreshFocus()
        rebuild()
    }

    func closeAll() { windows.map(\.id).forEach(close) }

    func focus(_ id: UUID) { focused = id; refreshFocus() }

    func focusNext(_ step: Int = 1) {
        guard !windows.isEmpty else { return }
        let i = windows.firstIndex { $0.id == focused } ?? -1
        focus(windows[((i + step) % windows.count + windows.count) % windows.count].id)
    }

    private func refreshFocus() {
        for w in windows { w.setFocused(w.id == focused) }
        objectWillChange.send()
    }

    func resize(_ id: UUID, by factor: Double) {
        guard let w = windows.first(where: { $0.id == id }) else { return }
        w.widthM = min(max(w.widthM * factor, 0.3), 4.0)
        rebuild()
    }

    func flip(_ id: UUID) { windows.first { $0.id == id }?.flip(); rebuild() }

    /// Games: the window fills the glasses pixel for pixel and stays with the
    /// head. Tap again for an ordinary window in the room.
    func toggleFill(_ id: UUID) {
        guard let w = windows.first(where: { $0.id == id }), let space else { return }
        if w.headLocked {
            w.headLocked = false
            w.resize(to: HostWindow.wide, widthM: 1.3)
        } else {
            w.headLocked = true
            w.pinned = nil
            w.resize(to: HostWindow.panel, widthM: Double(perch_space_fill_width(space, distance)))
        }
        rebuild()
    }

    /// Put the focused window where the wearer is looking now.
    func pinFocused() {
        guard let space, let i = windows.firstIndex(where: { $0.id == focused }), !windows[i].headLocked else { return }
        var p = [Float](repeating: 0, count: 5)
        if perch_space_pin_here(space, Int32(i)) == 0, perch_space_placement(space, Int32(i), &p) == 0 {
            windows[i].pinned = (p[0], p[1])
            objectWillChange.send()
        }
    }

    /// The pop-up's back button, as on a phone: leave fullscreen if the page
    /// is in it, otherwise go back one page; with nowhere to go, send Escape
    /// so a dialog or menu can close.
    func back() {
        guard let w = focusedWindow else { return }
        w.leaveFullscreen { wasFullscreen in
            if wasFullscreen { return }
            if w.web.canGoBack { w.web.goBack() } else { w.key("Escape", code: 27) }
        }
    }

    /// Swap the focused window with its neighbour in the row: -1 = with the
    /// one on its left, +1 = on its right. Both give up their pins.
    func moveFocused(by step: Int) {
        guard let i = windows.firstIndex(where: { $0.id == focused }) else { return }
        let j = i + step
        guard windows.indices.contains(j) else { return }
        windows[i].pinned = nil; windows[j].pinned = nil
        windows.swapAt(i, j)
        rebuild()
    }

    /// The row again: every window side by side, centred straight ahead.
    func arrange() { windows.forEach { $0.pinned = nil }; rebuild() }

    func moveLayout(by delta: Float) {
        distance = min(max(distance + delta, 1.0), 4.0)
        defaults.set(Double(distance), forKey: "host.distance")
        rebuild()
    }

    /// Recreate the Rust workspace from the window list (the C API has no
    /// remove): the windows hang in a row, then pinned ones go back to their
    /// pins and head-locked ones to the centre of the view.
    private func rebuild() {
        if let space { perch_space_free(space) }
        space = perch_space_new(UInt32(screenSize.width), UInt32(screenSize.height), Float(fovDeg * .pi / 180), 0)
        guard let space else { return }
        func upsert(_ i: Int32, _ w: HostWindow, yaw: Float, pitch: Float) {
            _ = perch_space_upsert(space, i, 2, UInt32(w.size.width), UInt32(w.size.height), yaw, pitch, distance,
                                   Float(w.widthM), Float(w.heightM), w.headLocked ? 1 : 0)
        }
        for w in windows { upsert(-1, w, yaw: 0, pitch: 0) }
        _ = perch_space_arrange_row(space, distance, gap, 0)
        for (i, w) in windows.enumerated() {
            if w.headLocked { upsert(Int32(i), w, yaw: 0, pitch: 0) } else if let pin = w.pinned { upsert(Int32(i), w, yaw: pin.yaw, pitch: pin.pitch) }
        }
        stage.setIdle(windows.isEmpty)
        UIApplication.shared.isIdleTimerDisabled = !windows.isEmpty
        objectWillChange.send()
        save()
    }

    /// Windows as the panel's layout map wants them.
    var infos: [WindowInfo] {
        guard let space else { return [] }
        return windows.enumerated().map { i, w in
            var p = [Float](repeating: 0, count: 5)
            _ = perch_space_placement(space, Int32(i), &p)
            return WindowInfo(id: w.id.uuidString, label: w.title, kind: "virtual", focused: w.id == focused, visible: true,
                              yawDeg: Double(p[0]) * 180 / .pi, pitchDeg: Double(p[1]) * 180 / .pi,
                              widthM: w.widthM, heightM: w.heightM, distance: Double(p[2]))
        }
    }

    // MARK: bookmarks and session
    func addBookmark(name: String, url: String) { bookmarks.add(name: name, url: url); saveBookmarks() }
    func removeBookmark(_ url: String) { bookmarks.remove(url: url); saveBookmarks() }
    private func saveBookmarks() { defaults.set(bookmarks.json, forKey: "host.bookmarks") }

    /// Remember the open pages so the next session starts where this one ended.
    private func save() {
        guard restored else { return }
        let pages = windows.compactMap { w -> [String: Any]? in
            guard let u = w.web.url, u.scheme == "http" || u.scheme == "https" else { return nil }
            return ["url": u.absoluteString, "tall": w.size.height > w.size.width]
        }
        defaults.set(pages, forKey: "host.windows")
    }

    private func restore() {
        for page in defaults.array(forKey: "host.windows") as? [[String: Any]] ?? [] {
            if let s = page["url"] as? String, let u = URL(string: s) { add(u, tall: page["tall"] as? Bool ?? false) }
        }
    }

    // MARK: frame
    fileprivate func tick(_ l: CADisplayLink) {
        guard let space else { return }
        let now = CACurrentMediaTime()
        let dt = lastTick > 0 ? Float(now - lastTick) : 0
        lastTick = now
        var pose: [Float] = [1, 0, 0, 0, 0, 0, 0, 1]
        // Aim at the moment this frame reaches the panel.
        let live = tracker.map { perch_ext_tracker_pose($0, l.targetTimestamp, &pose) == 0 } ?? false
        if !live { pose = [1, 0, 0, 0, 0, 0, 0, 0] }
        _ = perch_space_set_pose(space, pose)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        var corners = [Float](repeating: 0, count: 8), h = [Float](repeating: 0, count: 9)
        for (i, w) in windows.enumerated() {
            let r = perch_space_project(space, Int32(i), dt, &corners, &h)
            w.container.isHidden = r < 0
            if r >= 0 {
                w.lastTransform = Self.transform(h, size: w.size)
                w.container.layer.transform = w.lastTransform
            }
        }
        CATransaction.commit()
        frames += 1
        if now - fpsStart >= 1 {
            let isTracking = live && now - lastSample < 1
            if fps != frames || tracking != isTracking { fps = frames; tracking = isTracking }
            frames = 0; fpsStart = now
        }
    }

    /// The core's homography maps the unit square to panel pixels; a layer is
    /// `size` points across, so fold that scale in and lay the 3×3 matrix out
    /// as the projective `CATransform3D` Core Animation applies to (x, y, 0, 1).
    private static func transform(_ h: [Float], size: CGSize) -> CATransform3D {
        let w = CGFloat(size.width), ht = CGFloat(size.height)
        var t = CATransform3DIdentity
        t.m11 = CGFloat(h[0]) / w; t.m21 = CGFloat(h[1]) / ht; t.m41 = CGFloat(h[2])
        t.m12 = CGFloat(h[3]) / w; t.m22 = CGFloat(h[4]) / ht; t.m42 = CGFloat(h[5])
        t.m14 = CGFloat(h[6]) / w; t.m24 = CGFloat(h[7]) / ht; t.m44 = CGFloat(h[8])
        return t
    }

    // MARK: input
    private var pressed = false

    /// Controller state (pop-up skin or a paired controller) for the focused page.
    func gamepad(axes: [Float], buttons: UInt16) {
        let g = StandardGamepad.map(axes: axes, buttons: buttons)
        focusedWindow?.gamepad(axes: g.axes, buttons: g.buttons)
    }

    private func apply(_ events: [Postcard.Event]) {
        guard let w = focusedWindow else { return }
        for e in events {
            switch e {
            case let .mouseMove(dx, dy): w.moveCursor(dx: CGFloat(dx), dy: CGFloat(dy), now: CACurrentMediaTime())
            case let .mouseButton(button, down):
                if button == Postcard.btnRight { if down { w.web.goBack() } }
                else if down { pressed = true } else if pressed { pressed = false; w.click() }
            case let .scroll(dx, dy):
                // Wheel lines (+ = up / right) → the page follows the fingers.
                w.scroll(dx: CGFloat(-dx) * 48, dy: CGFloat(-dy) * 48)
            case let .text(t): w.type(t)
            case let .key(usage, down, mods):
                guard down else { continue }
                switch usage {
                case Hid.enter: w.enter()
                case Hid.backspace: w.backspace()
                case Hid.escape: w.key("Escape", code: 27)
                case Hid.tab: w.key("Tab", code: 9)
                case Hid.up: w.key("ArrowUp", code: 38)
                case Hid.down: w.key("ArrowDown", code: 40)
                case Hid.left: w.key("ArrowLeft", code: 37)
                case Hid.right: w.key("ArrowRight", code: 39)
                default:
                    // A hardware keyboard: plain and shifted keys type; chords are not passed on.
                    if mods & ~2 == 0, let c = KeyMap.character(usage: usage, shift: mods & 2 != 0) { w.type(c) }
                }
            case let .gamepad(axes, buttons): gamepad(axes: axes, buttons: buttons)
            }
        }
    }
}

/// `CADisplayLink` retains its target; this keeps the host out of that cycle.
private final class DisplayTick: NSObject {
    weak var host: GlassesHost?
    init(_ host: GlassesHost) { self.host = host }
    @objc func tick(_ l: CADisplayLink) { MainActor.assumeIsolated { host?.tick(l) } }
}

/// The root of the window on the glasses: black, the windows on top, a dimmer
/// over them, and a card while there are none.
final class StageController: UIViewController {
    private let idle = UIHostingController(rootView: IdleCard())
    private let dimmer = UIView()

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        addChild(idle)
        idle.view.frame = view.bounds
        idle.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        idle.view.backgroundColor = .black
        view.insertSubview(idle.view, at: 0)
        idle.didMove(toParent: self)
        dimmer.frame = view.bounds
        dimmer.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        dimmer.backgroundColor = .black
        dimmer.isUserInteractionEnabled = false
        dimmer.layer.zPosition = 10_000   // above every window, whatever order they were added in
        view.addSubview(dimmer)
    }

    func setIdle(_ on: Bool) { loadViewIfNeeded(); idle.view.isHidden = !on }
    func setDim(_ amount: Double) { loadViewIfNeeded(); dimmer.alpha = min(max(amount, 0), 0.85) }
    override var prefersHomeIndicatorAutoHidden: Bool { true }
}

private struct IdleCard: View {
    var body: some View {
        ZStack {
            Color.black
            VStack(spacing: 12) {
                Text("Perch").font(.system(size: 64, weight: .bold))
                Text("Add a window on the iPad").font(.system(size: 28))
            }
            .foregroundStyle(.white.opacity(0.85))
        }
        .ignoresSafeArea()
    }
}
