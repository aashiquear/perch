import PerchCore
import SwiftUI
import UIKit

/// The trackpad surface. Fingers go through `TouchpadGestures` (PerchCore);
/// a trackpad or mouse attached to the iPad drives the laptop's pointer
/// directly: hover moves, the button presses, two-finger scroll scrolls.
struct TouchpadView: UIViewRepresentable {
    let remote: RemoteInput
    let dragLock: Bool
    /// Width of the rim where a stroke starts a drag, points.
    let edge: CGFloat

    func makeUIView(context: Context) -> TrackpadUIView {
        let v = TrackpadUIView()
        v.gestures.target = remote
        v.remote = remote
        return v
    }

    func updateUIView(_ v: TrackpadUIView, context: Context) {
        v.gestures.dragLock = dragLock
        v.gestures.edge = Float(edge)
    }
}

final class TrackpadUIView: UIView {
    let gestures = TouchpadGestures()
    weak var remote: RemoteInput?
    /// Fingers on the pad, oldest first; the oldest is the one that is tracked.
    private var fingers: [UITouch] = []
    private var lastHover: CGPoint?
    private var pointerDown = false
    private var wheelAcc = CGPoint.zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        backgroundColor = .clear
        addGestureRecognizer(UIHoverGestureRecognizer(target: self, action: #selector(onHover(_:))))
        // Scroll wheel / two fingers on an attached trackpad: no touches, only this.
        let wheel = UIPanGestureRecognizer(target: self, action: #selector(onWheel(_:)))
        wheel.allowedScrollTypesMask = .all
        wheel.allowedTouchTypes = []
        addGestureRecognizer(wheel)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    override func layoutSubviews() {
        super.layoutSubviews()
        gestures.padW = Float(bounds.width)
        gestures.padH = Float(bounds.height)
    }

    // MARK: attached pointer device
    @objc private func onHover(_ g: UIHoverGestureRecognizer) {
        let pt = g.location(in: self)
        if g.state == .changed, let l = lastHover {
            remote?.cursorMove(dx: Float(pt.x - l.x) * 1.6, dy: Float(pt.y - l.y) * 1.6)
        }
        lastHover = (g.state == .began || g.state == .changed) ? pt : nil
    }

    @objc private func onWheel(_ g: UIPanGestureRecognizer) {
        let t = g.translation(in: self)
        g.setTranslation(.zero, in: self)
        if g.state == .began { wheelAcc = .zero }
        wheelAcc.x += t.x; wheelAcc.y += t.y
        let step: CGFloat = 14
        let lx = (wheelAcc.x / step).rounded(.towardZero), ly = (wheelAcc.y / step).rounded(.towardZero)
        if lx != 0 || ly != 0 {
            wheelAcc.x -= lx * step; wheelAcc.y -= ly * step
            remote?.scroll(dx: Float(lx), dy: Float(ly))
        }
    }

    // MARK: touches
    private func isPointer(_ t: UITouch) -> Bool { t.type == .indirectPointer }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches {
            if isPointer(t) {
                if event?.buttonMask.contains(.secondary) == true { remote?.rightTap() }
                else { pointerDown = true; remote?.pressDown() }
                lastHover = t.location(in: self)
                continue
            }
            fingers.append(t)
            let first = fingers[0].location(in: self)
            gestures.down(count: fingers.count, x: Float(first.x), y: Float(first.y), t: t.timestamp)
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches where isPointer(t) {
            let pt = t.location(in: self)
            if let l = lastHover { remote?.cursorMove(dx: Float(pt.x - l.x) * 1.6, dy: Float(pt.y - l.y) * 1.6) }
            lastHover = pt
        }
        guard let first = fingers.first, touches.contains(first) else { return }
        // Coalesced touches: every sample the digitiser took, not one per frame.
        for c in event?.coalescedTouches(for: first) ?? [first] {
            let pt = c.location(in: self)
            gestures.move(count: fingers.count, x: Float(pt.x), y: Float(pt.y))
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for t in touches {
            if isPointer(t) {
                if pointerDown { pointerDown = false; remote?.pressUp() }
                continue
            }
            guard let i = fingers.firstIndex(of: t) else { continue }
            fingers.remove(at: i)
            if i == 0, let next = fingers.first {
                // The tracked finger left: carry on from the next one without a jump.
                let pt = next.location(in: self)
                gestures.down(count: fingers.count, x: Float(pt.x), y: Float(pt.y), t: t.timestamp)
            }
            gestures.up(remaining: fingers.count, t: t.timestamp)
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        if pointerDown { pointerDown = false; remote?.pressUp() }
        fingers.removeAll()
        gestures.cancel()
    }
}

/// Catches the hardware keyboard while the pop-up is up and forwards every
/// key, down and up, to the host. `UIKey.keyCode` is the USB HID usage perchd
/// expects, so there is no table.
struct KeyCatcher: UIViewRepresentable {
    let remote: RemoteInput
    let cmdAsCtrl: Bool
    /// False while the typing field has the keyboard.
    let active: Bool

    func makeUIView(context: Context) -> KeyCatcherView {
        let v = KeyCatcherView()
        v.remote = remote
        return v
    }

    func updateUIView(_ v: KeyCatcherView, context: Context) {
        v.cmdAsCtrl = cmdAsCtrl
        v.active = active
        if active, v.window != nil, !v.isFirstResponder { DispatchQueue.main.async { _ = v.becomeFirstResponder() } }
    }
}

final class KeyCatcherView: UIView {
    weak var remote: RemoteInput?
    var cmdAsCtrl = true
    var active = true
    private var held: [Int: Int] = [:]   // usage we sent → mods, for the release

    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil, active { DispatchQueue.main.async { [weak self] in _ = self?.becomeFirstResponder() } }
    }

    private func forward(_ presses: Set<UIPress>, down: Bool) -> Bool {
        var handled = false
        for press in presses {
            guard let key = press.key else { continue }
            let f = key.modifierFlags
            let usage = KeyMap.usage(key.keyCode.rawValue, cmdAsCtrl: cmdAsCtrl)
            let mods = KeyMap.mods(ctrl: f.contains(.control), shift: f.contains(.shift), alt: f.contains(.alternate),
                                   cmd: f.contains(.command), cmdAsCtrl: cmdAsCtrl)
            if down { held[usage] = mods } else if held.removeValue(forKey: usage) == nil { continue }
            remote?.key(usage, down: down, mods: mods)
            handled = true
        }
        return handled
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !forward(presses, down: true) { super.pressesBegan(presses, with: event) }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        if !forward(presses, down: false) { super.pressesEnded(presses, with: event) }
    }

    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        // The app lost the keyboard (⌘Tab…): nothing may stay held on the host.
        for (usage, mods) in held { remote?.key(usage, down: false, mods: mods) }
        held.removeAll()
        super.pressesCancelled(presses, with: event)
    }

    override func resignFirstResponder() -> Bool {
        for (usage, mods) in held { remote?.key(usage, down: false, mods: mods) }
        held.removeAll()
        return super.resignFirstResponder()
    }
}

/// A multi-touch surface for one control: reports where its finger is, or nil
/// when it lifts. UIKit delivers touches to each of these views separately,
/// so a stick and a button can be held at the same time.
struct TouchSurface: UIViewRepresentable {
    /// (location, size of the surface); location is nil on release.
    let onChange: (CGPoint?, CGSize) -> Void

    func makeUIView(context: Context) -> SurfaceView { SurfaceView() }
    func updateUIView(_ v: SurfaceView, context: Context) { v.onChange = onChange }

    final class SurfaceView: UIView {
        var onChange: ((CGPoint?, CGSize) -> Void)?
        private var finger: UITouch?

        override init(frame: CGRect) {
            super.init(frame: frame)
            backgroundColor = .clear
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError("not used") }

        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard finger == nil, let t = touches.first else { return }
            finger = t
            onChange?(t.location(in: self), bounds.size)
        }

        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard let t = finger, touches.contains(t) else { return }
            onChange?(t.location(in: self), bounds.size)
        }

        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard let t = finger, touches.contains(t) else { return }
            finger = nil
            onChange?(nil, bounds.size)
        }

        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
            touchesEnded(touches, with: event)
        }
    }
}
