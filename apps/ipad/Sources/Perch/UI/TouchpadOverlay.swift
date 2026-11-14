import PerchCore
import SwiftUI

/// The pop-up over the panel: touchpad, keyboard, arrow pad and the three
/// controller skins, all aimed at the host's focused window. Same parts in
/// the same order as the Android `TouchpadOverlay`. It is a plain overlay,
/// not a sheet: a sheet's drag-to-dismiss would swallow vertical strokes.
/// What the pop-up is aimed at: a laptop running perchd, or the windows this
/// iPad draws on the glasses. Same UI, two destinations (Android: `InputTarget`).
struct PadContext {
    struct Item: Identifiable { let id: String; let label: String; let focused: Bool }
    let remote: RemoteInput
    let title: String
    let windows: [Item]
    let focus: (String) -> Void
    let focusNext: () -> Void
    let pin: () -> Void
    let recenter: () -> Void
    /// Size of the focused window and distance of the layout, where the target has them.
    var bigger: (() -> Void)?
    var smaller: (() -> Void)?
    var nearer: (() -> Void)?
    var further: (() -> Void)?
    /// What the first control button does where "back" means more than the
    /// Escape key (a page: leave fullscreen, else go back one page).
    var back: (() -> Void)?
    /// Swap the focused window with its neighbour (-1 left, +1 right).
    var move: ((Int) -> Void)?
    /// Named layouts for the Layout chip; empty hides it.
    var presets: [String] = []
    var applyPreset: (String) -> Void = { _ in }
    /// Controller skins (they need a host that presents a gamepad).
    var skins = true
    var escLabel = "esc"
    var hint = "2 fingers: scroll · 2-finger tap: right click · 3-finger swipe: next window"
}

struct TouchpadOverlay: View {
    @Environment(\.palette) private var p
    @Environment(\.horizontalSizeClass) private var sizeClass
    let ctx: PadContext
    @Binding var mode: Int              // 0 touchpad, 1 xbox, 2 wheel, 3 flight
    let cmdAsCtrl: Bool
    let onDismiss: () -> Void

    @State private var dragLock = false
    @State private var scrollLock = false
    @State private var typing = false
    @State private var moving = false   // the arrow pad moves the focused window
    @State private var scrollV = 0.0    // -1..1, the spring-back scrollers
    @State private var scrollH = 0.0

    private var remote: RemoteInput { ctx.remote }
    private let rim: CGFloat = 22

    var body: some View {
        VStack(spacing: 6) {
            // Input surface chips, close at the end.
            HStack(spacing: 4) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(Array((ctx.skins ? ["Touchpad", "Xbox", "Wheel", "Flight"] : ["Touchpad"]).enumerated()), id: \.offset) { i, n in
                            Chip(label: n, selected: mode == i) { mode = i }
                        }
                    }
                }
                Button(action: onDismiss) { Image(systemName: "xmark").font(.body.weight(.semibold)).frame(width: 40, height: 40) }
                    .buttonStyle(.plain).accessibilityLabel("close touchpad")
            }
            // The host and its windows; tap one to focus it.
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    Text(ctx.title).font(.subheadline.weight(.semibold)).padding(.trailing, 6)
                    ForEach(ctx.windows) { w in
                        Chip(label: w.label, selected: w.focused) { ctx.focus(w.id) }
                    }
                    if !ctx.presets.isEmpty {
                        Menu {
                            ForEach(ctx.presets, id: \.self) { name in
                                Button(name) { ctx.applyPreset(name) }
                            }
                        } label: { ChipLabel(label: "Layout", selected: false) }
                    }
                }
            }
            if mode == 0 {
                pad.frame(maxHeight: .infinity)
            } else {
                GamepadSkin(mode: mode, scale: sizeClass == .regular ? 1.45 : 1).padding(.vertical, 6)
            }
            controls
            if typing { TypeBar(remote: remote) { typing = false } }
            if mode != 0 { Spacer(minLength: 0) }
        }
        .padding(.horizontal, 12).padding(.top, 6).padding(.bottom, 8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(KeyCatcher(remote: remote, cmdAsCtrl: cmdAsCtrl, active: !typing).frame(width: 1, height: 1).opacity(0.01))
        .background(
            UnevenRoundedRectangle(topLeadingRadius: 24, topTrailingRadius: 24)
                .fill(p.surface).shadow(radius: 8).ignoresSafeArea(edges: .bottom)
        )
        .environment(\.padInput, ctx.remote)
        .defersSystemGestures(on: .all)
        .onChange(of: mode, initial: true) { _, m in if m != 0 { remote.announce() } }
        .task(id: scrollLock) {
            // Scroll-lock scrollers: displacement → wheel lines, ticking while held.
            while scrollLock, !Task.isCancelled {
                if scrollV != 0 || scrollH != 0 {
                    // Quadratic: fine near the middle, fast at the ends (≈ 25 lines/s).
                    func curve(_ v: Double) -> Float { Float(v * abs(v) * 0.8) }
                    remote.scroll(dx: curve(scrollH), dy: -curve(scrollV))
                }
                try? await Task.sleep(nanoseconds: 32_000_000)
            }
        }
    }

    // MARK: pad
    private var pad: some View {
        VStack(spacing: 4) {
            HStack(spacing: 4) {
                ZStack {
                    RoundedRectangle(cornerRadius: 16).fill(p.surfaceVariant)
                    // Thin rim: a stroke that starts here is a drag.
                    RoundedRectangle(cornerRadius: 16)
                        .strokeBorder(Color(hex: dragLock ? 0x3F51B5 : 0x2196F3, alpha: dragLock ? 0.33 : 0.2), lineWidth: rim)
                    Text(dragLock
                         ? "DRAG MODE: slide = drag with the button held\ntap the lock to go back"
                         : "1 finger move · tap · rim or double-tap+hold: drag\n" + ctx.hint)
                        .font(.footnote).multilineTextAlignment(.center).foregroundStyle(p.onSurfaceVariant)
                        .padding(.horizontal, 60)
                    TouchpadView(remote: remote, dragLock: dragLock, edge: rim)
                    // Locks in the corners: drag mode and the scrollers.
                    VStack {
                        HStack {
                            lock(scrollLock, "arrow.up.arrow.down", "scroll lock") { scrollLock.toggle(); scrollV = 0; scrollH = 0 }
                            Spacer()
                            lock(dragLock, dragLock ? "lock.fill" : "lock.open.fill", "drag lock") { dragLock.toggle() }
                        }
                        Spacer()
                    }
                    .padding(6)
                }
                if scrollLock { ScrollBar(vertical: true) { scrollV = $0 }.frame(width: 30) }
            }
            if scrollLock { ScrollBar(vertical: false) { scrollH = $0 }.frame(height: 30) }
        }
    }

    private func lock(_ on: Bool, _ icon: String, _ label: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 16)).frame(width: 40, height: 40)
                .foregroundStyle(on ? p.onPrimary : p.onSurface)
                .background(on ? p.primary : p.surface, in: Circle())
        }
        .buttonStyle(.plain).accessibilityLabel(label)
    }

    // MARK: controls — buttons wrap on the left, the arrow pad keeps the right
    private var controls: some View {
        HStack(alignment: .top, spacing: 8) {
            FlowRow {
                Small(icon: "arrow.backward", label: ctx.escLabel) { if let back = ctx.back { back() } else { remote.key(Hid.escape) } }
                Small(icon: "forward.end.fill", label: "next window") { ctx.focusNext() }
                Small(icon: "pin.fill", label: "pin here") { ctx.pin() }
                Small(icon: "scope", label: "recenter") { ctx.recenter() }
                if let f = ctx.bigger { Small(icon: "plus.magnifyingglass", label: "bigger", action: f) }
                if let f = ctx.smaller { Small(icon: "minus.magnifyingglass", label: "smaller", action: f) }
                if let f = ctx.nearer { Small(icon: "arrow.down.to.line", label: "nearer", action: f) }
                if let f = ctx.further { Small(icon: "arrow.up.to.line", label: "further", action: f) }
                if ctx.move != nil { Small(icon: "arrow.left.arrow.right", label: "move window", selected: moving) { moving.toggle() } }
            }
            .frame(maxWidth: .infinity)
            VStack(spacing: 4) {
                DPad { dir in arrow(dir) }.frame(width: 112, height: 112)
                Small(icon: "keyboard", label: "keyboard", selected: typing) { typing.toggle() }
            }
        }
    }

    /// Arrow keys, or the controller's d-pad while a skin is up.
    private func arrow(_ d: DPad.Direction) {
        if moving, let move = ctx.move {
            // Move mode: left / right swap the focused window with its neighbour; the centre ends it.
            switch d {
            case .left: move(-1)
            case .right: move(1)
            case .center: moving = false
            default: break
            }
            return
        }
        if mode != 0 {
            switch d {
            case .up: remote.hat(x: 0, y: -1)
            case .down: remote.hat(x: 0, y: 1)
            case .left: remote.hat(x: -1, y: 0)
            case .right: remote.hat(x: 1, y: 0)
            case .center: remote.button(.a, down: true); remote.button(.a, down: false); return
            }
            remote.hat(x: 0, y: 0)
        } else {
            switch d {
            case .up: remote.key(Hid.up)
            case .down: remote.key(Hid.down)
            case .left: remote.key(Hid.left)
            case .right: remote.key(Hid.right)
            case .center: remote.key(Hid.enter)
            }
        }
    }
}

/// Round four-way pad.
struct DPad: View {
    enum Direction { case up, down, left, right, center }
    @Environment(\.palette) private var p
    let onKey: (Direction) -> Void

    var body: some View {
        ZStack {
            Circle().fill(p.surfaceVariant)
            arm("chevron.up", .up).frame(maxHeight: .infinity, alignment: .top)
            arm("chevron.down", .down).frame(maxHeight: .infinity, alignment: .bottom)
            arm("chevron.left", .left).frame(maxWidth: .infinity, alignment: .leading)
            arm("chevron.right", .right).frame(maxWidth: .infinity, alignment: .trailing)
            arm("smallcircle.filled.circle", .center)
        }
    }

    private func arm(_ icon: String, _ d: Direction) -> some View {
        Button { onKey(d) } label: { Image(systemName: icon).frame(width: 36, height: 36).contentShape(Rectangle()) }
            .buttonStyle(.plain)
    }
}

/// A text field whose content is forwarded, as you type, to the host's
/// focused window: the on-screen keyboard for the laptop.
struct TypeBar: View {
    @Environment(\.palette) private var p
    let remote: RemoteInput
    let onClose: () -> Void
    @State private var text = ""
    @State private var sent = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 6) {
            TextField("type here → focused window", text: $text)
                .textInputAutocapitalization(.never).autocorrectionDisabled()
                .submitLabel(.send).focused($focused)
                .onSubmit { enter(); focused = true }
                .onChange(of: text) { _, new in
                    remote.typed(old: sent, new: new)
                    sent = new
                    if new.count > 120 { sent = ""; text = "" }
                }
                .padding(10).background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 10))
            Button(action: enter) { Image(systemName: "return").frame(width: 40, height: 40).background(p.surfaceVariant, in: Circle()) }
                .buttonStyle(.plain).accessibilityLabel("enter")
            Button(action: onClose) { Image(systemName: "xmark").frame(width: 36, height: 36) }
                .buttonStyle(.plain).accessibilityLabel("close keyboard")
        }
        .onAppear { focused = true }
    }

    private func enter() {
        remote.key(Hid.enter)
        sent = ""; text = ""
    }
}

/// A spring-back scroller: hold the knob away from the middle and the content
/// scrolls that way, faster the further; let go and it recentres.
struct ScrollBar: View {
    @Environment(\.palette) private var p
    let vertical: Bool
    let onValue: (Double) -> Void
    @State private var pos = 0.0     // -1..1 along the bar

    var body: some View {
        GeometryReader { geo in
            let length = vertical ? geo.size.height : geo.size.width
            let half = max(length / 2 - 14, 1)
            ZStack {
                RoundedRectangle(cornerRadius: 15).fill(p.surfaceVariant)
                Circle().fill(p.primary).frame(width: 28, height: 28)
                    .offset(x: vertical ? 0 : pos * half, y: vertical ? pos * half : 0)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { g in
                        let at = vertical ? g.location.y : g.location.x
                        pos = min(max((at - length / 2) / half, -1), 1)
                        onValue(pos)
                    }
                    .onEnded { _ in pos = 0; onValue(0) }
            )
        }
    }
}
