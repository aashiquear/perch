import SwiftUI

/// The Android app's colours (`ui/Theme.kt`). Dark-first: this UI is used next
/// to a pair of glasses in a dim room. High contrast, no pure white.
struct Palette {
    let primary: Color
    let onPrimary: Color
    let secondaryContainer: Color
    let background: Color
    let surface: Color
    let surfaceVariant: Color
    let onSurface: Color
    let onSurfaceVariant: Color
    let primaryContainer: Color
    let error: Color

    static let dark = Palette(
        primary: Color(hex: 0x8FD3FF), onPrimary: Color(hex: 0x00344F), secondaryContainer: Color(hex: 0x2A4A39),
        background: Color(hex: 0x0B0F14), surface: Color(hex: 0x121820), surfaceVariant: Color(hex: 0x1B2430),
        onSurface: Color(hex: 0xE2E6EC), onSurfaceVariant: Color(hex: 0x9AA7B8),
        primaryContainer: Color(hex: 0x004B70), error: Color(hex: 0xFFB4AB)
    )
    static let light = Palette(
        primary: Color(hex: 0x005E8C), onPrimary: .white, secondaryContainer: Color(hex: 0xC9EFD3),
        background: Color(hex: 0xFCFCFF), surface: Color(hex: 0xF1F4F9), surfaceVariant: Color(hex: 0xDDE3EA),
        onSurface: Color(hex: 0x1A1C1E), onSurfaceVariant: Color(hex: 0x41474D),
        primaryContainer: Color(hex: 0xC6E7FF), error: Color(hex: 0xBA1A1A)
    )
}

extension Color {
    init(hex: UInt32, alpha: Double = 1) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255,
                  blue: Double(hex & 0xff) / 255, opacity: alpha)
    }
}

private struct PaletteKey: EnvironmentKey { static let defaultValue = Palette.dark }
extension EnvironmentValues {
    var palette: Palette {
        get { self[PaletteKey.self] }
        set { self[PaletteKey.self] = newValue }
    }
}

/// 0 = follow the device, 1 = light, 2 = dark. Kept in UserDefaults ("theme").
enum ThemeMode {
    static let key = "theme"
    static func label(_ m: Int) -> String { m == 1 ? "Light" : (m == 2 ? "Dark" : "Device") }
    static func icon(_ m: Int) -> String { m == 1 ? "sun.max.fill" : (m == 2 ? "moon.fill" : "circle.lefthalf.filled") }
    static func scheme(_ m: Int) -> ColorScheme? { m == 1 ? .light : (m == 2 ? .dark : nil) }
}

/// Applies the stored theme and hands the matching palette to the views below.
struct PerchTheme<Content: View>: View {
    @AppStorage(ThemeMode.key) private var mode = 0
    @ViewBuilder var content: () -> Content

    var body: some View {
        Themed(content: content).preferredColorScheme(ThemeMode.scheme(mode))
    }

    private struct Themed: View {
        @Environment(\.colorScheme) private var scheme
        @ViewBuilder var content: () -> Content
        var body: some View {
            let p = scheme == .dark ? Palette.dark : Palette.light
            content()
                .environment(\.palette, p)
                .tint(p.primary)
                .foregroundStyle(p.onSurface)
                .background(p.background.ignoresSafeArea())
        }
    }
}
