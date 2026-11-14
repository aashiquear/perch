import SwiftUI
import UIKit

/// iPad / iPhone are *controllers*: iPadOS gives apps no access to the
/// glasses' USB IMU interface, so the glasses stay plugged into the laptop or
/// the Android phone and this app drives that host: the same panel and pop-up
/// (touchpad, keyboard, controller) as the Android app, over the network.
@main
struct PerchApp: App {
    @StateObject private var client = PerchClient(
        deviceName: UIDevice.current.name,
        platform: UIDevice.current.userInterfaceIdiom == .pad ? "i_pad_os" : "ios"
    )

    var body: some Scene {
        WindowGroup {
            PerchTheme { RootView() }.environmentObject(client)
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var client: PerchClient
    @Environment(\.scenePhase) private var phase

    var body: some View {
        // The Android app's two tabs.
        TabView(selection: $tab) {
            HostScreen()
                .tabItem { Label("Glasses", systemImage: "eyeglasses") }.tag(0)
            Group {
                if client.connected { PanelScreen() } else { ConnectScreen() }
            }
            .tabItem { Label("Laptop remote", systemImage: "laptopcomputer") }.tag(1)
        }
        .onAppear { client.startDiscovery(); HostLab.shared.snapshotScenes() }
        .onChange(of: phase) { _, new in
            // iOS closes sockets of a suspended app; come back to the same host.
            if new == .active, !client.connected, !client.connecting, !client.lastHost.isEmpty, wasConnected {
                client.connect(host: client.lastHost)
            }
            if new == .background { wasConnected = client.connected }
        }
    }

    @State private var wasConnected = false
    @AppStorage("tab") private var tab = 1
}
