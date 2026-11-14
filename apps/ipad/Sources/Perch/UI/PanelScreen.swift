import PerchCore
import SwiftUI

/// The dashboard: the iPad twin of the Android panel (`PanelScreen.kt`) and
/// the Linux control panel. Same order — header, stat tiles, layout map,
/// actions two per row, presets, brightness, hardware-input switch, window
/// cards — and the same pop-up for touchpad, keyboard and controller. The
/// difference is where things happen: on the host this iPad is connected to.
struct PanelScreen: View {
    @EnvironmentObject private var client: PerchClient
    @Environment(\.palette) private var p
    @AppStorage(ThemeMode.key) private var themeMode = 0
    @AppStorage("cmdAsCtrl") private var cmdAsCtrl = true
    @State private var touchpad = false
    @State private var padMode = 0          // 0 touchpad, 1 xbox, 2 wheel, 3 flight
    @State private var arrange = false
    @State private var brightness = 80.0

    private var s: HostState { client.state }

    var body: some View {
        ZStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    header
                    HStack(spacing: 8) {
                        Stat(value: "\(s.windows.count)", caption: "windows")
                        Stat(value: "\(s.peers)", caption: "devices")
                        Stat(value: s.displayMode ?? "–", caption: "display mode")
                    }
                    LayoutMap(windows: s.mapOrder) { client.focus($0) }
                    actions
                    if arrange { presets }
                    brightnessRow
                    Toggle(isOn: $cmdAsCtrl) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Keyboard · trackpad · mouse → laptop").font(.body.weight(.semibold))
                            Text("while the touchpad is open, a keyboard, trackpad or mouse attached to this iPad drives the laptop. On: ⌘ acts as Ctrl (⌘C, ⌘V…). Off: ⌘ is sent as the Super key.")
                                .font(.footnote).foregroundStyle(p.onSurfaceVariant)
                        }
                    }
                    .tint(p.primary)
                    if let e = client.lastError { Text(e).font(.footnote).foregroundStyle(p.error) }
                    windowList
                }
                .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 24)
                .frame(maxWidth: 760)
                .frame(maxWidth: .infinity)
            }
            if touchpad {
                TouchpadOverlay(ctx: padContext, mode: $padMode, cmdAsCtrl: cmdAsCtrl) { touchpad = false }
                    .transition(.move(edge: .bottom))
            }
        }
        .animation(.easeOut(duration: 0.2), value: touchpad)
    }

    private var padContext: PadContext {
        PadContext(
            remote: client.remote, title: client.hostName,
            windows: s.windows.filter(\.visible).map { .init(id: $0.id, label: $0.label, focused: $0.focused) },
            focus: { client.focus($0) }, focusNext: { client.focusNext() }, pin: { client.pinFocused() },
            recenter: { client.recenter() },
            nearer: { client.moveLayout(by: -0.25) }, further: { client.moveLayout(by: 0.25) },
            presets: ["single", "coding", "gaming", "couch"], applyPreset: { client.preset($0) }
        )
    }

    // MARK: header
    private var header: some View {
        HStack(spacing: 12) {
            if s.glasses != nil { GlassesImage().frame(height: 72) }
            VStack(alignment: .leading, spacing: 2) {
                Text("Perch").font(.largeTitle.bold())
                Text("\(s.glasses ?? "No glasses") on \(client.hostName)").font(.subheadline)
                Text(s.glasses != nil ? "glasses connected · \(s.calibration.label)" : "glasses not plugged into the host")
                    .font(.footnote).foregroundStyle(p.onSurfaceVariant)
                if s.glasses != nil, s.calibration == .uncalibrated {
                    Text("Not calibrated: windows will drift. Put the glasses on a table and tap Calibrate once.")
                        .font(.footnote).foregroundStyle(p.error)
                }
                Text(touchpad ? "● TOUCHPAD — moving the cursor on the laptop" : "○ PANEL")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color(hex: touchpad ? 0x5ADC8C : 0xE6BE50))
            }
        }
    }

    // MARK: actions — always two per row, no holes
    private var actions: some View {
        VStack(spacing: 10) {
            HStack(spacing: 8) {
                Menu {
                    Button("Full · 1920×1080") { client.moveToGlasses(width: 1920, height: 1080) }
                    Button("Half · 960×1080") { client.moveToGlasses(width: 960, height: 1080) }
                    Button("Wide · 2560×1080") { client.moveToGlasses(width: 2560, height: 1080) }
                    Button("Tall · 1080×1440") { client.moveToGlasses(width: 1080, height: 1440) }
                } label: {
                    ActionLabel(icon: "rectangle.badge.plus", label: "To glasses", hint: "the laptop's focused window")
                }
                Action(icon: "hand.point.up.left.fill", label: "Touchpad", hint: "cursor on the laptop", selected: touchpad && padMode == 0) {
                    padMode = 0; touchpad = true
                }
            }
            HStack(spacing: 8) {
                Action(icon: "gamecontroller.fill", label: "Gamepad", hint: "Xbox · wheel · flight") {
                    if padMode == 0 { padMode = 1 }
                    touchpad = true
                }
                Action(icon: "scope", label: "Recenter", hint: "temple button") { client.recenter() }
            }
            HStack(spacing: 8) {
                Action(icon: "slider.horizontal.3", label: "Calibrate", hint: "20 s, glasses still") { client.calibrate(seconds: 20) }
                Action(icon: ThemeMode.icon(themeMode), label: "Theme", hint: "\(ThemeMode.label(themeMode)) · tap to change") {
                    themeMode = (themeMode + 1) % 3
                }
            }
            HStack(spacing: 8) {
                Action(icon: "square.grid.2x2.fill", label: arrange ? "Arrange ▴" : "Arrange ▾", hint: "presets", selected: arrange) { arrange.toggle() }
                Action(icon: "laptopcomputer", label: "All back", hint: "release every window") { client.removeAll() }
            }
            HStack(spacing: 8) {
                Menu {
                    ForEach(["mono120", "mono", "stereo120", "stereo"], id: \.self) { m in
                        Button(m) { client.setMode(m) }
                    }
                } label: {
                    ActionLabel(icon: "display", label: "Display", hint: "\(s.displayMode ?? "mode") · tap to change")
                }
                Action(icon: "wifi.slash", label: "Disconnect", hint: client.hostName) { client.disconnect() }
            }
        }
    }

    private var presets: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                PresetPicture(name: "re-flow", ratios: [1, 1.78, 1]) { client.arrange() }
                PresetPicture(name: "single", ratios: [1.78]) { client.preset("single") }
                PresetPicture(name: "coding", ratios: [1.22, 1.78, 1.22]) { client.preset("coding") }
                PresetPicture(name: "gaming", ratios: [1.78]) { client.preset("gaming") }
                PresetPicture(name: "couch", ratios: [1.78]) { client.preset("couch") }
                ForEach(s.layouts, id: \.self) { name in
                    Chip(label: name, selected: false) { client.loadLayout(name) }
                }
            }
        }
    }

    /// Brightness of the glasses themselves, set through the host.
    private var brightnessRow: some View {
        HStack(spacing: 8) {
            Text("glasses").font(.caption2)
            Image(systemName: "sun.min").font(.footnote)
            Slider(value: $brightness, in: 0...100, step: 5) { editing in
                if !editing { client.setBrightness(Int(brightness)) }
            }
            Image(systemName: "sun.max.fill").font(.footnote)
            Text("\(Int(brightness))%").font(.caption2).frame(width: 40, alignment: .trailing)
        }
    }

    // MARK: windows
    @ViewBuilder private var windowList: some View {
        Text("Windows (\(s.windows.count))").font(.headline)
        if s.windows.isEmpty {
            Text("None yet. Focus a window on the laptop, then tap To glasses.").font(.footnote)
        }
        ForEach(s.windows) { w in
            VStack(spacing: 4) {
                HStack(spacing: 10) {
                    Image(systemName: icon(for: w.kind)).font(.title3).frame(width: 36, height: 36)
                    VStack(alignment: .leading, spacing: 1) {
                        Text((w.focused ? "● " : "○ ") + w.label).font(.body.weight(.semibold)).lineLimit(1)
                        Text(String(format: "%@ · %.1f × %.1f m · %.0f° %@%@", w.portrait ? "portrait" : "landscape",
                                    w.widthM, w.heightM, abs(w.yawDeg), w.yawDeg >= 0 ? "left" : "right", w.visible ? "" : " · hidden"))
                            .font(.footnote)
                    }
                    Spacer(minLength: 0)
                    iconButton("pin.fill", "pin where I look") { client.pinHere(w.id) }
                    iconButton("xmark", "back to the laptop") { client.remove(w.id) }
                }
                if w.focused {
                    FlowRow {
                        Small(icon: "arrow.left.and.right.square", label: "narrow") { client.setSpan(w.id, 1) }
                        Small(icon: "rectangle", label: "normal") { client.setSpan(w.id, 2) }
                        Small(icon: "rectangle.expand.vertical", label: "wide") { client.setSpan(w.id, 3) }
                        Small(icon: "wand.and.stars", label: "auto width") { client.setSpan(w.id, 0) }
                        Small(icon: "arrow.down", label: "nearer") { client.moveLayout(by: -0.25) }
                        Small(icon: "arrow.up", label: "further") { client.moveLayout(by: 0.25) }
                        Small(icon: "xmark.app", label: "close app") { client.closeApp(w.id) }
                    }
                    .padding(.top, 2)
                }
            }
            .padding(10)
            .background(w.focused ? p.primaryContainer : p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
            .contentShape(Rectangle())
            .onTapGesture { client.focus(w.id) }
        }
    }

    private func iconButton(_ name: String, _ label: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: name).frame(width: 40, height: 40).contentShape(Rectangle()) }
            .buttonStyle(.plain).accessibilityLabel(label)
    }

    private func icon(for kind: String) -> String {
        switch kind {
        case "display": return "display"
        case "window": return "macwindow"
        case "virtual": return "rectangle.dashed"
        case "dex", "phone_screen": return "iphone"
        default: return "square.grid.2x2"
        }
    }
}
