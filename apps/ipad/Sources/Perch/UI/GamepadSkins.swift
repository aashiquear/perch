import PerchCore
import QuartzCore
import SwiftUI

/// The controller surface inside the pop-up. Three skins over one virtual
/// gamepad (the host presents an Xbox-class controller): Xbox layout, racing
/// wheel (wheel → left stick X, pedals → triggers, paddles → bumpers), flight
/// stick (stick → left stick, rudder → right stick X, throttle → right stick
/// Y). Port of the Android `GamepadOverlay.kt`; sizes are its dp × `scale`.
struct GamepadSkin: View {
    let mode: Int
    let scale: CGFloat

    var body: some View {
        switch mode {
        case 1: XboxSkin(k: scale)
        case 2: WheelSkin(k: scale)
        default: FlightSkin(k: scale)
        }
    }
}

/// Where the skins send the controller state (set by the pop-up).
private struct PadInputKey: EnvironmentKey { static let defaultValue = RemoteInput { _ in } }
extension EnvironmentValues {
    var padInput: RemoteInput {
        get { self[PadInputKey.self] }
        set { self[PadInputKey.self] = newValue }
    }
}

private extension RemoteInput {
    /// Continuous motion: rate-limited.
    func axis(_ i: Int, _ v: Float) { setAxes([(i, v)], force: false, now: CACurrentMediaTime()) }
    /// Sticks and releases: always sent.
    func axesNow(_ v: [(Int, Float)]) { setAxes(v, force: true, now: CACurrentMediaTime()) }
}

// MARK: controls

/// Round stick; emits normalised (x, y) with y positive = down.
private struct Stick: View {
    @Environment(\.palette) private var p
    let size: CGFloat
    let label: String
    let onMove: (Float, Float) -> Void
    @State private var knob = CGSize.zero
    @State private var start: CGPoint?

    var body: some View {
        ZStack {
            Circle().fill(p.surfaceVariant)
            Text(label).font(.caption2).foregroundStyle(p.onSurfaceVariant).frame(maxHeight: .infinity, alignment: .top).padding(4)
            Circle().fill(p.primary).frame(width: size / 2.6, height: size / 2.6).offset(knob)
            TouchSurface { pt, sz in
                guard let pt else { start = nil; knob = .zero; onMove(0, 0); return }
                let s = start ?? pt
                start = s
                let r = sz.width / 2
                var dx = pt.x - s.x, dy = pt.y - s.y
                let d = hypot(dx, dy)
                if d > r { dx = dx / d * r; dy = dy / d * r }
                knob = CGSize(width: dx, height: dy)
                onMove(Float(dx / r), Float(dy / r))
            }
        }
        .frame(width: size, height: size)
    }
}

/// Press-and-hold controller button.
private struct PadKey: View {
    @Environment(\.padInput) private var pad
    @Environment(\.palette) private var p
    let label: String
    let button: PadButton
    var size: CGFloat = 52
    var color: Color?
    var radius: CGFloat?        // nil = round
    @State private var pressed = false

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: radius ?? size / 2).fill(color ?? p.secondaryContainer)
                .brightness(pressed ? -0.18 : 0)
            Text(label).font(.system(size: min(size * 0.32, 20), weight: .bold))
                .foregroundStyle(color == nil ? p.onSurface : Color.black.opacity(0.8))
            TouchSurface { pt, _ in
                let down = pt != nil
                guard down != pressed else { return }
                pressed = down
                pad.button(button, down: down)
            }
        }
        .frame(width: size, height: size)
    }
}

/// Four-way hat on the hat axes.
private struct Hat: View {
    @Environment(\.padInput) private var pad
    @Environment(\.palette) private var p
    let size: CGFloat
    @State private var last = CGPoint.zero

    var body: some View {
        ZStack {
            Circle().fill(p.surfaceVariant)
            Image(systemName: "plus").font(.system(size: size * 0.3, weight: .bold))
            TouchSurface { pt, sz in
                var h = CGPoint.zero
                if let pt {
                    let ang = atan2(pt.y - sz.height / 2, pt.x - sz.width / 2)
                    h.x = abs(cos(ang)) > 0.5 ? (cos(ang) > 0 ? 1 : -1) : 0
                    h.y = abs(sin(ang)) > 0.5 ? (sin(ang) > 0 ? 1 : -1) : 0
                }
                guard h != last else { return }
                last = h
                pad.axesNow([(6, Float(h.x)), (7, Float(h.y))])
            }
        }
        .frame(width: size, height: size)
    }
}

/// Vertical pedal / throttle; 0..1 from bottom to top.
private struct Pedal: View {
    @Environment(\.palette) private var p
    let label: String
    let width: CGFloat
    let height: CGFloat
    var springBack = true
    let onValue: (Float) -> Void
    @State private var v: CGFloat = 0

    var body: some View {
        ZStack(alignment: .bottom) {
            RoundedRectangle(cornerRadius: 12).fill(p.surfaceVariant)
            Rectangle().fill(p.primary.opacity(0.6)).frame(height: max(v, 0.02) * height)
            Text(label).font(.caption2).frame(maxHeight: .infinity, alignment: .top).padding(4)
            TouchSurface { pt, sz in
                if let pt { v = min(max(1 - pt.y / sz.height, 0), 1) } else if springBack { v = 0 } else { return }
                onValue(Float(v))
            }
        }
        .frame(width: width, height: height)
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

// MARK: skins

private struct XboxSkin: View {
    @Environment(\.padInput) private var pad
    let k: CGFloat

    var body: some View {
        let r = pad
        VStack(spacing: 6 * k) {
            // Triggers and bumpers at the corners, the Xbox button in the middle.
            HStack {
                HStack(spacing: 6) {
                    PadKey(label: "LT", button: .l2, size: 56 * k, radius: 12)
                    PadKey(label: "LB", button: .l1, size: 56 * k, radius: 12)
                }
                Spacer()
                PadKey(label: "ⓧ", button: .mode, size: 40 * k, color: Color(hex: 0x9CCC65))
                Spacer()
                HStack(spacing: 6) {
                    PadKey(label: "RB", button: .r1, size: 56 * k, radius: 12)
                    PadKey(label: "RT", button: .r2, size: 56 * k, radius: 12)
                }
            }
            // The two sticks, stick-click buttons beside them.
            HStack {
                Spacer()
                HStack(alignment: .bottom, spacing: 4) {
                    PadKey(label: "L3", button: .thumbL, size: 34 * k)
                    Stick(size: 118 * k, label: "L") { x, y in r.axesNow([(0, x), (1, y)]) }
                }
                Spacer()
                HStack(alignment: .bottom, spacing: 4) {
                    Stick(size: 118 * k, label: "R") { x, y in r.axesNow([(2, x), (3, y)]) }
                    PadKey(label: "R3", button: .thumbR, size: 34 * k)
                }
                Spacer()
            }
            // View / menu and the hat on the left, A B X Y on the right.
            HStack {
                HStack(spacing: 6) {
                    VStack(spacing: 4) {
                        PadKey(label: "view", button: .select, size: 40 * k, radius: 10)
                        PadKey(label: "menu", button: .start, size: 40 * k, radius: 10)
                    }
                    Hat(size: 84 * k)
                }
                Spacer()
                // Diamond on a tight 3 × 3 grid: buttons touch but never overlap.
                let btn = 54 * k, cell = 52 * k
                ZStack(alignment: .topLeading) {
                    PadKey(label: "Y", button: .y, size: btn, color: Color(hex: 0xF9D342)).offset(x: cell, y: 0)
                    PadKey(label: "X", button: .x, size: btn, color: Color(hex: 0x6FA8DC)).offset(x: 0, y: cell)
                    PadKey(label: "B", button: .b, size: btn, color: Color(hex: 0xE57373)).offset(x: cell * 2, y: cell)
                    PadKey(label: "A", button: .a, size: btn, color: Color(hex: 0x81C784)).offset(x: cell, y: cell * 2)
                }
                .frame(width: cell * 2 + btn, height: cell * 2 + btn, alignment: .topLeading)
            }
        }
        .frame(maxWidth: 820)
    }
}

private struct WheelSkin: View {
    @Environment(\.padInput) private var pad
    @Environment(\.palette) private var p
    let k: CGFloat
    @State private var angle = 0.0          // degrees, ±180 = full lock
    @State private var lastTouch: Double?

    var body: some View {
        let r = pad
        VStack(spacing: 6 * k) {
            HStack {
                VStack(spacing: 6) {
                    PadKey(label: "L", button: .l1, size: 48 * k, radius: 10)
                    Pedal(label: "brake", width: 56 * k, height: 140 * k) { r.axis(4, $0) }
                }
                Spacer()
                ZStack {
                    Circle().fill(p.surfaceVariant)
                    Circle().fill(p.primary).frame(width: 28 * k, height: 28 * k)
                        .offset(x: 76 * k * cos((angle - 90) * .pi / 180), y: 76 * k * sin((angle - 90) * .pi / 180))
                    Text("\(Int(angle))°").font(.caption2)
                    TouchSurface { pt, sz in
                        guard let pt else { lastTouch = nil; angle = 0; r.axesNow([(0, 0)]); return }
                        let a = atan2(pt.y - sz.height / 2, pt.x - sz.width / 2) * 180 / .pi
                        if let l = lastTouch {
                            var d = a - l
                            if d > 180 { d -= 360 }
                            if d < -180 { d += 360 }
                            angle = min(max(angle + d, -180), 180)
                            r.axis(0, Float(angle / 180))
                        }
                        lastTouch = a
                    }
                }
                .frame(width: 190 * k, height: 190 * k)
                Spacer()
                VStack(alignment: .trailing, spacing: 6) {
                    PadKey(label: "R", button: .r1, size: 48 * k, radius: 10)
                    Pedal(label: "gas", width: 56 * k, height: 140 * k) { r.axis(5, $0) }
                }
            }
            HStack {
                PadKey(label: "A", button: .a, size: 54 * k, color: Color(hex: 0x81C784)); Spacer()
                PadKey(label: "B", button: .b, size: 54 * k, color: Color(hex: 0xE57373)); Spacer()
                PadKey(label: "X", button: .x, size: 54 * k, color: Color(hex: 0x6FA8DC)); Spacer()
                PadKey(label: "Y", button: .y, size: 54 * k, color: Color(hex: 0xF9D342)); Spacer()
                PadKey(label: "view", button: .select, size: 46 * k, radius: 10); Spacer()
                PadKey(label: "menu", button: .start, size: 46 * k, radius: 10)
            }
        }
        .frame(maxWidth: 820)
    }
}

private struct FlightSkin: View {
    @Environment(\.padInput) private var pad
    let k: CGFloat
    @State private var rudder = 0.0

    var body: some View {
        let r = pad
        HStack {
            // The throttle holds its position (no spring-back), like a real quadrant.
            Pedal(label: "throttle", width: 64 * k, height: 200 * k, springBack: false) { r.axis(3, -($0 * 2 - 1)) }
            Spacer()
            VStack(spacing: 2) {
                Stick(size: 170 * k, label: "stick") { x, y in r.axesNow([(0, x), (1, y)]) }
                Slider(value: $rudder, in: -1...1) { editing in
                    if !editing { rudder = 0; r.axesNow([(2, 0)]) }
                }
                .frame(width: 170 * k)
                .onChange(of: rudder) { _, v in r.axis(2, Float(v)) }
                Text("rudder").font(.caption2)
            }
            Spacer()
            VStack(spacing: 6) {
                Hat(size: 76 * k)
                PadKey(label: "FIRE", button: .a, size: 68 * k, color: Color(hex: 0xE57373))
                HStack(spacing: 6) {
                    PadKey(label: "B", button: .b, size: 48 * k)
                    PadKey(label: "X", button: .x, size: 48 * k)
                }
                HStack(spacing: 6) {
                    PadKey(label: "L1", button: .l1, size: 48 * k, radius: 10)
                    PadKey(label: "R1", button: .r1, size: 48 * k, radius: 10)
                }
            }
        }
        .frame(maxWidth: 820)
    }
}
