import PerchCore
import SwiftUI

/// The "Glasses" tab: this iPad drives the glasses itself. The same panel as
/// the Android host — header, stat tiles, layout map, actions two per row,
/// brightness, window cards, pop-up touchpad and controller — with what
/// iPadOS allows underneath: windows are pages and files the app renders (it
/// cannot show other apps), and head motion comes from Bose AR headphones.
struct HostScreen: View {
    @StateObject private var host = GlassesHost.shared
    @StateObject private var bose = HostLab.shared.bose
    @Environment(\.palette) private var p
    @AppStorage(ThemeMode.key) private var themeMode = 0
    @AppStorage("host.useBose") private var useBose = false
    @State private var touchpad = false
    @State private var padMode = 0          // 0 touchpad, 1 xbox, 2 wheel, 3 flight
    @State private var adding = false
    @State private var checking = false
    @State private var onIPad: HostWindow?

    var body: some View {
        ZStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    header
                    HStack(spacing: 8) {
                        Stat(value: "\(host.windows.count)", caption: "windows")
                        Stat(value: bose.hz > 0 ? String(format: "%.0f", bose.hz) : "–", caption: "tracker Hz")
                        Stat(value: host.displayAttached ? "\(host.fps)" : "–", caption: "glasses fps")
                    }
                    LayoutMap(windows: host.infos.sorted { $0.yawDeg > $1.yawDeg }) { id in
                        if let u = UUID(uuidString: id) { host.focus(u) }
                    }
                    BookmarkBar(bookmarks: host.bookmarks.items, onOpen: { host.open($0) }, onOpenNew: { host.add($0) },
                                onRemove: { host.removeBookmark($0) })
                    actions
                    brightnessRow
                    SoundRow()
                    Toggle(isOn: $host.realTouches) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Genuine taps").font(.body.weight(.semibold))
                            Text("a tap on the glasses is delivered as a real touch, which sites such as Xbox Cloud insist on. Turn off if taps misbehave; they are then performed by script.")
                                .font(.footnote).foregroundStyle(p.onSurfaceVariant)
                        }
                    }
                    .tint(p.primary)
                    windowList
                }
                .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 24)
                .frame(maxWidth: 760)
                .frame(maxWidth: .infinity)
            }
            if touchpad {
                TouchpadOverlay(ctx: padContext, mode: $padMode, cmdAsCtrl: false) { touchpad = false }
                    .transition(.move(edge: .bottom))
            }
        }
        .animation(.easeOut(duration: 0.2), value: touchpad)
        .sheet(isPresented: $adding) { AddWindowSheet(bookmarks: host.bookmarks.items) { url, tall in host.add(url, tall: tall) } }
        .sheet(isPresented: $checking) { PerchTheme { HostCheckScreen() } }
        .fullScreenCover(item: $onIPad) { OnIPadSheet(window: $0) }
        .onAppear {
            HostLab.shared.snapshotScenes()
            if useBose, !bose.running { bose.start() }
        }
    }

    private var padContext: PadContext {
        PadContext(
            remote: host.input, title: "Glasses",
            windows: host.windows.map { .init(id: $0.id.uuidString, label: $0.title, focused: $0.id == host.focused) },
            focus: { if let u = UUID(uuidString: $0) { host.focus(u) } },
            focusNext: { host.focusNext() }, pin: { host.pinFocused() }, recenter: { host.recenter() },
            bigger: { if let id = host.focused { host.resize(id, by: 1.15) } },
            smaller: { if let id = host.focused { host.resize(id, by: 1 / 1.15) } },
            nearer: { host.moveLayout(by: -0.25) }, further: { host.moveLayout(by: 0.25) },
            back: { host.back() }, move: { host.moveFocused(by: $0) },
            escLabel: "back",
            hint: "2 fingers: scroll · 2-finger tap: page back · 3-finger swipe: next window"
        )
    }

    private var header: some View {
        HStack(spacing: 12) {
            if host.displayAttached { GlassesImage().frame(height: 72) }
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("Perch").font(.largeTitle.bold())
                    Spacer()
                    Button { themeMode = (themeMode + 1) % 3 } label: {
                        Image(systemName: ThemeMode.icon(themeMode)).font(.title3).frame(width: 40, height: 40)
                    }
                    .buttonStyle(.plain).accessibilityLabel("theme: \(ThemeMode.label(themeMode))")
                }
                Text(host.displayAttached ? "Glasses on this iPad" : "Plug the glasses into the iPad").font(.subheadline)
                Text(trackerLine).font(.footnote).foregroundStyle(p.onSurfaceVariant)
                if host.displayAttached, !host.tracking {
                    Text("No head tracking: windows stay in front of your eyes. Switch on the Bose headphones and tap Head tracking.")
                        .font(.footnote).foregroundStyle(p.error)
                }
                Text(touchpad ? "● TOUCHPAD — moving the cursor on the glasses" : "○ PANEL")
                    .font(.footnote.weight(.semibold)).foregroundStyle(Color(hex: touchpad ? 0x5ADC8C : 0xE6BE50))
            }
        }
    }

    private var trackerLine: String {
        let display = host.displayAttached ? "glasses display attached" : "no glasses display"
        let head = bose.hz > 0 ? String(format: "Bose headphones %.0f Hz", bose.hz) : (bose.running ? "looking for the headphones…" : "head tracking off")
        return "\(display) · \(head)"
    }

    // MARK: actions — always two per row
    private var actions: some View {
        VStack(spacing: 10) {
            HStack(spacing: 8) {
                Action(icon: "rectangle.badge.plus", label: "Add window", hint: "page, video or file") { adding = true }
                Action(icon: "hand.point.up.left.fill", label: "Touchpad", hint: "cursor in focused window", selected: touchpad && padMode == 0) {
                    padMode = 0; touchpad = true
                }
            }
            HStack(spacing: 8) {
                Action(icon: "gamecontroller.fill", label: "Gamepad", hint: "Xbox · wheel · flight", selected: touchpad && padMode != 0) {
                    if padMode == 0 { padMode = 1 }
                    touchpad = true
                }
                Action(icon: "scope", label: "Recenter", hint: "look straight, head level") { host.recenter() }
            }
            HStack(spacing: 8) {
                Action(icon: "headphones", label: "Head tracking", hint: bose.running ? "Bose · tap to stop" : "Bose headphones", selected: bose.running) {
                    if bose.running { bose.stop(); useBose = false } else { bose.start(); useBose = true }
                }
                Action(icon: "square.grid.2x2.fill", label: "Arrange", hint: "side by side, centred") { host.arrange() }
            }
            HStack(spacing: 8) {
                Action(icon: "xmark.rectangle", label: "Close all", hint: "every window") { host.closeAll() }
                Action(icon: "checklist", label: "Check", hint: "display · USB · headphones") { checking = true }
            }
        }
    }

    /// Software dimming of the glasses picture.
    private var brightnessRow: some View {
        HStack(spacing: 8) {
            Text("glasses").font(.caption2)
            Image(systemName: "sun.min").font(.footnote)
            Slider(value: $host.brightness, in: 0.15...1)
            Image(systemName: "sun.max.fill").font(.footnote)
            Text("\(Int(host.brightness * 100))%").font(.caption2).frame(width: 40, alignment: .trailing)
        }
    }

    // MARK: windows
    @ViewBuilder private var windowList: some View {
        Text("Windows (\(host.windows.count))").font(.headline)
        if host.windows.isEmpty {
            Text("None yet. Tap a bookmark or Add window → it opens on the glasses.").font(.footnote)
        }
        ForEach(host.windows) { w in
            let isFocused = w.id == host.focused
            VStack(spacing: 6) {
                HStack(spacing: 10) {
                    Image(systemName: w.headLocked ? "gamecontroller" : "macwindow").font(.title3).frame(width: 36, height: 36)
                    VStack(alignment: .leading, spacing: 1) {
                        Text((isFocused ? "● " : "○ ") + w.title).font(.body.weight(.semibold)).lineLimit(1)
                        Text(String(format: "%@ · %.0f×%.0f · %@", w.subtitle, w.size.width, w.size.height,
                                    w.headLocked ? "fills the view, follows the head" : (w.pinned != nil ? "pinned" : String(format: "%.1f m wide", w.widthM))))
                            .font(.footnote).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    iconButton("pin.fill", "pin where I look") { host.focus(w.id); host.pinFocused() }
                    iconButton("xmark", "close") { host.close(w.id) }
                }
                if isFocused {
                    AddressBar(window: w, isBookmarked: host.bookmarks.contains(url: w.web.url?.absoluteString ?? ""),
                               onGo: { w.load($0) },
                               onBookmark: {
                                   guard let u = w.web.url?.absoluteString, !(w.web.url?.isFileURL ?? true) else { return }
                                   if host.bookmarks.contains(url: u) { host.removeBookmark(u) } else { host.addBookmark(name: w.title, url: u) }
                               })
                        .id(w.id)
                    FlowRow {
                        Small(icon: "chevron.backward", label: "back") { w.web.goBack() }
                        Small(icon: "chevron.forward", label: "forward") { w.web.goForward() }
                        Small(icon: "arrow.clockwise", label: "reload") { w.web.reload() }
                        Small(icon: "ipad.landscape", label: "on iPad") { onIPad = w }
                        Small(icon: "arrow.up.left.and.arrow.down.right", label: w.headLocked ? "window" : "fill view", selected: w.headLocked) { host.toggleFill(w.id) }
                        if !w.headLocked { Small(icon: "rectangle.portrait.rotate", label: "tall / wide") { host.flip(w.id) } }
                    }
                }
            }
            .padding(10)
            .background(isFocused ? p.primaryContainer : p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
            .contentShape(Rectangle())
            .onTapGesture { host.focus(w.id) }
        }
        if !host.windows.isEmpty {
            Text("Sign-in: tap “on iPad” to use the page on this screen with the keyboard and your saved passwords, then send it back to the glasses. You stay signed in.")
                .font(.footnote).foregroundStyle(p.onSurfaceVariant)
        }
    }

    private func iconButton(_ name: String, _ label: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: name).frame(width: 40, height: 40).contentShape(Rectangle()) }
            .buttonStyle(.plain).accessibilityLabel(label)
    }
}
