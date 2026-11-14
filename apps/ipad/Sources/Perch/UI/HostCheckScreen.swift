import SwiftUI

/// The "Glasses" tab: the iPad driving the glasses itself, as the Android
/// app does. iPadOS decides whether that is possible, so this screen first
/// measures it on the device — display, the glasses' motion sensor, and the
/// earphone alternative — and reports what it found.
struct HostCheckScreen: View {
    @EnvironmentObject private var client: PerchClient
    @StateObject private var lab = HostLab.shared
    @StateObject private var bose = HostLab.shared.bose
    @Environment(\.palette) private var p
    @State private var laptop = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    GlassesImage().frame(height: 72)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Check this iPad").font(.largeTitle.bold())
                        Text("What iPadOS allows a glasses host").font(.subheadline)
                    }
                }
                Text("Plug the glasses into the iPad's USB-C port with this screen open, then tap Run check. A card should appear in the glasses. For head tracking without the glasses' sensor: switch the Bose 700 on, wear it, tap Bose headphones and follow the three marks.")
                    .font(.callout).foregroundStyle(p.onSurfaceVariant)

                HStack(spacing: 8) {
                    Action(icon: "checklist", label: "Run check", hint: "display + USB sensor") {
                        lab.snapshotScenes(); lab.checkUsb()
                    }
                    Action(icon: "headphones", label: bose.running ? "Stop Bose" : "Bose headphones", hint: "head tracking over Bluetooth", selected: bose.running) {
                        if bose.running { bose.stop() } else { bose.start() }
                    }
                }
                if bose.hz > 0 {
                    Text("Data is arriving. Sit as you would on a plane and tap each mark while holding the pose:").font(.callout)
                    HStack(spacing: 8) {
                        Chip(label: "1 · looking straight", selected: false) { bose.mark("straight") }
                        Chip(label: "2 · head turned LEFT", selected: false) { bose.mark("left") }
                        Chip(label: "3 · looking UP", selected: false) { bose.mark("up") }
                    }
                }

                section("1 · Display", ok: lab.externalConnected,
                        verdict: lab.externalConnected ? "the app is drawing on the glasses" : "no external display handed to the app yet",
                        lines: lab.displayLines)
                section("2 · Glasses motion sensor (USB)", ok: lab.usbUsable,
                        verdict: lab.usbUsable ? "readable" : "not readable by an app (so far)", lines: lab.usbLines)
                section("3 · Bose headphones (Bluetooth)", ok: bose.hz > 0,
                        verdict: bose.hz > 0 ? String(format: "%.0f Hz", bose.hz) : "no samples", lines: bose.lines)
                section("4 · AirPods head tracking", ok: lab.headHz > 0,
                        verdict: lab.headHz > 0 ? String(format: "%.0f Hz · yaw %+.0f° pitch %+.0f°", lab.headHz, lab.headYawDeg, lab.headPitchDeg) : "no samples",
                        lines: lab.headLines)

                Text("Send the result").font(.headline).padding(.top, 6)
                HStack(spacing: 8) {
                    TextField("laptop IP", text: $laptop)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.numbersAndPunctuation)
                        .padding(12).background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
                    Button { lab.send(to: laptop.trimmingCharacters(in: .whitespaces)) } label: {
                        Text("Send to laptop").font(.body.weight(.semibold)).padding(.horizontal, 18).padding(.vertical, 10)
                            .foregroundStyle(p.onPrimary).background(p.primary, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    ShareLink(item: lab.report) { Image(systemName: "square.and.arrow.up").frame(width: 40, height: 40) }
                }
                if !lab.sendStatus.isEmpty { Text(lab.sendStatus).font(.footnote).foregroundStyle(p.onSurfaceVariant) }
            }
            .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 24)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .onAppear { if laptop.isEmpty { laptop = client.lastHost }; lab.snapshotScenes() }
    }

    private func section(_ title: String, ok: Bool, verdict: String, lines: [String]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                Text((ok ? "● " : "○ ") + verdict).font(.footnote.weight(.semibold))
                    .foregroundStyle(Color(hex: ok ? 0x5ADC8C : 0xE6BE50))
            }
            Text(lines.joined(separator: "\n")).font(.system(.caption, design: .monospaced))
                .foregroundStyle(p.onSurfaceVariant).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(12).background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
    }
}
