import SwiftUI

/// Shown until a host answers: hosts found on the network, or an address.
struct ConnectScreen: View {
    @EnvironmentObject private var client: PerchClient
    @Environment(\.palette) private var p
    @State private var manual = ""
    @State private var token = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    GlassesImage().frame(height: 72)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Perch").font(.largeTitle.bold())
                        Text("Remote for the glasses on your laptop or phone").font(.subheadline)
                    }
                }
                Text("This iPad is the panel, trackpad, keyboard and game controller for a computer running perchd with the glasses plugged in. Both must be on the same Wi-Fi.")
                    .font(.callout).foregroundStyle(p.onSurfaceVariant)

                Text("Hosts on this network").font(.headline).padding(.top, 8)
                if client.hosts.isEmpty {
                    HStack(spacing: 10) {
                        ProgressView()
                        Text("Looking for perchd… start it on your laptop.").font(.callout).foregroundStyle(p.onSurfaceVariant)
                    }
                }
                ForEach(client.hosts) { h in
                    Button { client.connect(to: h) } label: {
                        HStack(spacing: 12) {
                            Image(systemName: "laptopcomputer").font(.title2).frame(width: 36)
                            Text(h.name).font(.body.weight(.semibold))
                            Spacer()
                            Image(systemName: "chevron.right").foregroundStyle(p.onSurfaceVariant)
                        }
                        .padding(14)
                        .background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
                    }
                    .buttonStyle(.plain)
                }

                Text("Or by address").font(.headline).padding(.top, 8)
                HStack(spacing: 8) {
                    TextField("host IP, e.g. 192.168.1.20", text: $manual)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.numbersAndPunctuation)
                        .submitLabel(.go).onSubmit(connectManual)
                        .padding(12).background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
                    TextField("pairing token (if set)", text: $token)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .padding(12).background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
                        .frame(maxWidth: 220)
                }
                Button(action: connectManual) {
                    Text(client.connecting ? "Connecting…" : "Connect").font(.body.weight(.semibold))
                        .padding(.horizontal, 22).padding(.vertical, 10)
                        .foregroundStyle(p.onPrimary)
                        .background(p.primary.opacity(manual.isEmpty ? 0.4 : 1), in: Capsule())
                }
                .buttonStyle(.plain).disabled(manual.isEmpty || client.connecting)

                if let e = client.lastError { Text(e).font(.callout).foregroundStyle(p.error) }
            }
            .padding(20)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .onAppear { if manual.isEmpty { manual = client.lastHost } }
    }

    private func connectManual() {
        let host = manual.trimmingCharacters(in: .whitespaces)
        guard !host.isEmpty else { return }
        client.connect(host: host, token: token.trimmingCharacters(in: .whitespaces))
    }
}

/// The Rokid Max picture shared with the Android and Linux panels.
struct GlassesImage: View {
    var body: some View {
        if let ui = UIImage(named: "rokid_max", in: .module, with: nil) {
            Image(uiImage: ui).resizable().scaledToFit()
        } else {
            Image(systemName: "eyeglasses").resizable().scaledToFit()
        }
    }
}
