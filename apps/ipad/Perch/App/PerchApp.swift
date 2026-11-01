import SwiftUI

@main
struct PerchApp: App {
    @StateObject private var client = PerchClient(deviceName: UIDevice.current.name)
    var body: some Scene {
        WindowGroup { ContentView().environmentObject(client).preferredColorScheme(.dark) }
    }
}

/// iPad/iPhone are *controllers only*: iPadOS does not expose the glasses'
/// USB IMU interface to apps (no libusb, no MFi for Rokid), so host mode is
/// not possible here. What the iPad is great at: a big, precise trackpad,
/// a hardware keyboard forwarder, and a window manager with a live map of
/// where your windows are.
struct ContentView: View {
    @EnvironmentObject var client: PerchClient
    @State private var manual = ""
    @State private var typed = ""

    var body: some View {
        Group {
            if client.connected { connected } else { picker }
        }
        .onAppear { client.startDiscovery() }
        .onDisappear { client.stopDiscovery() }
    }

    private var picker: some View {
        NavigationStack {
            List {
                Section("Perch hosts on this network") {
                    if client.hosts.isEmpty { Text("Looking for perchd… run it on your laptop.").foregroundStyle(.secondary) }
                    ForEach(client.hosts) { h in Button(h.name) { client.connect(to: h) } }
                }
                Section("Manual") {
                    TextField("host IP", text: $manual).textInputAutocapitalization(.never).autocorrectionDisabled()
                    Button("Connect") { client.connect(host: manual.trimmingCharacters(in: .whitespaces)) }.disabled(manual.isEmpty)
                }
                if let e = client.lastError { Section { Text(e).foregroundStyle(.red) } }
            }
            .navigationTitle("Perch")
        }
    }

    private var connected: some View {
        VStack(spacing: 10) {
            HStack {
                VStack(alignment: .leading) {
                    Text(client.hostName).font(.headline)
                    Text(client.glasses ?? "glasses not connected").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Disconnect") { client.disconnect() }
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack {
                    ForEach(client.windows.filter(\.visible)) { w in
                        Button { client.focus(w.id) } label: { Text(w.label).padding(.horizontal, 12).padding(.vertical, 6) }
                            .buttonStyle(.bordered).tint(w.focused ? .accentColor : .gray)
                            .contextMenu { Button("Pin here") { client.pinHere(w.id) } }
                    }
                }
            }
            HStack {
                Button("Recenter") { client.recenter() }.buttonStyle(.borderedProminent)
                Button("Pin here") { if let f = client.windows.first(where: \.focused) { client.pinHere(f.id) } }.buttonStyle(.bordered)
                Button("Next") { client.focusNext() }.buttonStyle(.bordered)
                Menu("Layout") { ForEach(["single", "coding", "gaming", "couch"], id: \.self) { p in Button(p) { client.preset(p) } } }.buttonStyle(.bordered)
                Menu("Display") { ForEach(["mono120", "mono", "stereo120", "stereo"], id: \.self) { m in Button(m) { client.setMode(m) } } }.buttonStyle(.bordered)
            }
            TouchpadView(client: client)
            HStack {
                TextField("type here → focused window", text: $typed)
                    .textFieldStyle(.roundedBorder)
                    .onChange(of: typed) { _, new in forwardTyping(new) }
                Button("⏎") { client.sendInput([.key(usage: 0x28, down: true, mods: 0), .key(usage: 0x28, down: false, mods: 0)]) }.buttonStyle(.bordered)
            }
        }
        .padding()
    }

    @State private var lastTyped = ""
    private func forwardTyping(_ new: String) {
        if new.count > lastTyped.count {
            let added = String(new.dropFirst(lastTyped.count))
            client.sendInput(added.map { .text(String($0)) })
        } else if new.count < lastTyped.count {
            client.sendInput([.key(usage: 0x2a, down: true, mods: 0), .key(usage: 0x2a, down: false, mods: 0)])
        }
        lastTyped = new.count > 200 ? "" : new
        if new.count > 200 { typed = "" }
    }
}
