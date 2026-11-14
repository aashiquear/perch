import Foundation

/// USB HID usages (page 7) the UI sends by name.
public enum Hid {
    public static let enter = 0x28, escape = 0x29, backspace = 0x2a, tab = 0x2b
    public static let right = 0x4f, left = 0x50, down = 0x51, up = 0x52
    public static let leftCtrl = 0xe0, leftGui = 0xe3, rightCtrl = 0xe4, rightGui = 0xe7
}

/// Controller buttons; the raw value is the bit in the wire `buttons` field
/// (docs/PROTOCOL.md, `gamepad`, since 1.6).
public enum PadButton: Int, CaseIterable {
    case a = 0, b, c, x, y, z, l1, r1, l2, r2, select, start, mode, thumbL, thumbR
}

/// Hardware-keyboard translation for a perchd host.
public enum KeyMap {
    /// perchd modifier bits: 1 ctrl, 2 shift, 4 alt, 8 gui. With `cmdAsCtrl`
    /// the Apple ⌘ key acts as Ctrl, so ⌘C / ⌘V do what the hand expects on a
    /// Linux or Windows host.
    public static func mods(ctrl: Bool, shift: Bool, alt: Bool, cmd: Bool, cmdAsCtrl: Bool) -> Int {
        var m = 0
        if ctrl || (cmd && cmdAsCtrl) { m |= 1 }
        if shift { m |= 2 }
        if alt { m |= 4 }
        if cmd && !cmdAsCtrl { m |= 8 }
        return m
    }

    /// The character a key types on a US layout, for hosts that take text
    /// (the glasses-on-iPad web windows) instead of key codes; nil for keys
    /// that type nothing.
    public static func character(usage u: Int, shift: Bool) -> String? {
        switch u {
        case 0x04...0x1d:
            let c = Character(UnicodeScalar(UInt8(ascii: "a") + UInt8(u - 0x04)))
            return shift ? String(c).uppercased() : String(c)
        case 0x1e...0x27:
            return String(Array(shift ? "!@#$%^&*()" : "1234567890")[u - 0x1e])
        case 0x2c: return " "
        case 0x2d...0x38 where u != 0x32:
            let plain: [Int: String] = [0x2d: "-", 0x2e: "=", 0x2f: "[", 0x30: "]", 0x31: "\\", 0x33: ";", 0x34: "'", 0x35: "`", 0x36: ",", 0x37: ".", 0x38: "/"]
            let shifted: [Int: String] = [0x2d: "_", 0x2e: "+", 0x2f: "{", 0x30: "}", 0x31: "|", 0x33: ":", 0x34: "\"", 0x35: "~", 0x36: "<", 0x37: ">", 0x38: "?"]
            return (shift ? shifted : plain)[u]
        default: return nil
        }
    }

    /// The usage to send for a pressed key (`UIKey.keyCode` is already a HID usage).
    public static func usage(_ u: Int, cmdAsCtrl: Bool) -> Int {
        guard cmdAsCtrl else { return u }
        switch u {
        case Hid.leftGui: return Hid.leftCtrl
        case Hid.rightGui: return Hid.rightCtrl
        default: return u
        }
    }
}

/// What the pop-up's touchpad, d-pad, keyboard and controller skins talk to.
/// The gesture recogniser only needs this, so it is tested without a network.
public protocol InputTarget: AnyObject {
    func cursorMove(dx: Float, dy: Float)
    func tap()
    func rightTap()
    func pressDown()
    func pressUp()
    /// Wheel lines: dy positive = up, dx positive = right.
    func scroll(dx: Float, dy: Float)
    func focusNext(_ step: Int)
}

/// The pop-up's input turned into stream events for a laptop running perchd.
/// Mirrors the Android `RemoteTarget`: same axes and button layout.
public final class RemoteInput: InputTarget {
    /// Ships one batch over UDP.
    public var send: ([Postcard.Event]) -> Void
    /// Asks the host to focus the next window.
    public var onFocusNext: () -> Void = {}

    /// lx ly rx ry (−1..1, y down), lt rt (0..1), hat x y (−1/0/1).
    public private(set) var axes = [Float](repeating: 0, count: 8)
    public private(set) var buttons: UInt16 = 0
    private var lastReport = -Double.infinity
    /// Minimum seconds between unforced axis reports.
    public var minInterval = 0.012

    public init(send: @escaping ([Postcard.Event]) -> Void) { self.send = send }

    // MARK: pointer
    public func cursorMove(dx: Float, dy: Float) { send([.mouseMove(dx: dx, dy: dy)]) }
    public func tap() { click(Postcard.btnLeft) }
    public func rightTap() { click(Postcard.btnRight) }
    public func pressDown() { send([.mouseButton(button: Postcard.btnLeft, down: true)]) }
    public func pressUp() { send([.mouseButton(button: Postcard.btnLeft, down: false)]) }
    public func scroll(dx: Float, dy: Float) { send([.scroll(dx: dx, dy: dy)]) }
    public func focusNext(_ step: Int) { onFocusNext() }
    private func click(_ b: Int) { send([.mouseButton(button: b, down: true), .mouseButton(button: b, down: false)]) }

    // MARK: keyboard
    /// Press and release.
    public func key(_ usage: Int, mods: Int = 0) {
        send([.key(usage: usage, down: true, mods: mods), .key(usage: usage, down: false, mods: mods)])
    }
    public func key(_ usage: Int, down: Bool, mods: Int) { send([.key(usage: usage, down: down, mods: mods)]) }
    public func typeText(_ text: String) {
        guard !text.isEmpty else { return }
        send(text.map { .text(String($0)) })
    }

    /// The typing field changed from `old` to `new`: send only the difference.
    public func typed(old: String, new: String) {
        if new.count > old.count, new.hasPrefix(old) {
            typeText(String(new.dropFirst(old.count)))
        } else if new.count < old.count, old.hasPrefix(new) {
            for _ in 0..<(old.count - new.count) { key(Hid.backspace) }
        }
        // A mid-string edit (autocorrect) is not replayed.
    }

    // MARK: controller
    /// Set axes; reports are rate-limited unless `force`.
    public func setAxes(_ values: [(Int, Float)], force: Bool, now: Double) {
        for (i, v) in values where i >= 0 && i < 8 {
            let lo: Float = (i == 4 || i == 5) ? 0 : -1
            axes[i] = min(max(v, lo), 1)
        }
        if !force && now - lastReport < minInterval { return }
        lastReport = now
        report()
    }

    public func button(_ b: PadButton, down: Bool) {
        let bit = UInt16(1) << UInt16(b.rawValue)
        if down { buttons |= bit } else { buttons &= ~bit }
        // The triggers are both a button and an analogue axis.
        if b == .l2 { axes[4] = down ? 1 : 0 }
        if b == .r2 { axes[5] = down ? 1 : 0 }
        report()
    }

    /// Hat as a d-pad press (used by the arrow pad while a controller skin is up).
    public func hat(x: Float, y: Float) { axes[6] = x; axes[7] = y; report() }

    /// All centred, nothing pressed: makes the host create its virtual controller.
    public func announce() {
        axes = [Float](repeating: 0, count: 8)
        buttons = 0
        report()
    }

    private func report() { send([.gamepad(axes: axes, buttons: buttons)]) }
}
