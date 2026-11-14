import AVKit
import MediaPlayer
import PerchCore
import SwiftUI
import UniformTypeIdentifiers

// Pieces of the Glasses tab that have no twin on the laptop-remote panel:
// the address bar, the bookmark bar, the add-window sheet and the sheet that
// brings a window onto the iPad's own screen.

/// Address of the focused window: shows where it is, takes where to go.
struct AddressBar: View {
    @Environment(\.palette) private var p
    let window: HostWindow
    let isBookmarked: Bool
    let onGo: (URL) -> Void
    let onBookmark: () -> Void
    @State private var text = ""
    @FocusState private var editing: Bool

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: window.web.url?.scheme == "https" ? "lock.fill" : "globe").font(.footnote).foregroundStyle(p.onSurfaceVariant)
            TextField("address, or words to search", text: $text)
                .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.webSearch)
                .submitLabel(.go).focused($editing)
                .onSubmit { if let u = WebAddress.url(for: text) { onGo(u) }; editing = false }
            if editing, !text.isEmpty {
                Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(p.onSurfaceVariant) }.buttonStyle(.plain)
            }
            Button(action: onBookmark) { Image(systemName: isBookmarked ? "star.fill" : "star").frame(width: 32, height: 32) }
                .buttonStyle(.plain).accessibilityLabel(isBookmarked ? "remove bookmark" : "bookmark this page")
        }
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(p.surface, in: RoundedRectangle(cornerRadius: 10))
        .onAppear { text = window.address }
        // Follow the page while the field is not being typed in.
        .onChange(of: window.address) { _, new in if !editing { text = new } }
        .onChange(of: editing) { _, now in if !now { text = window.address } }
    }
}

/// Saved sites: tap opens in the focused window, hold for more.
struct BookmarkBar: View {
    @Environment(\.palette) private var p
    let bookmarks: [Bookmark]
    let onOpen: (URL) -> Void
    let onOpenNew: (URL) -> Void
    let onRemove: (String) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                Image(systemName: "star.fill").font(.footnote).foregroundStyle(p.onSurfaceVariant)
                ForEach(bookmarks) { b in
                    Chip(label: b.name, selected: false) { if let u = URL(string: b.url) { onOpen(u) } }
                        .contextMenu {
                            Button { if let u = URL(string: b.url) { onOpenNew(u) } } label: { Label("Open in a new window", systemImage: "plus.rectangle") }
                            Button(role: .destructive) { onRemove(b.url) } label: { Label("Remove", systemImage: "trash") }
                        }
                }
                if bookmarks.isEmpty { Text("Star a page to keep it here").font(.footnote).foregroundStyle(p.onSurfaceVariant) }
            }
        }
    }
}

/// Choose what a new window shows: an address, a search, a bookmark, or a
/// file from the iPad (films and documents downloaded for a flight).
struct AddWindowSheet: View {
    @Environment(\.dismiss) private var dismiss
    let bookmarks: [Bookmark]
    let onPick: (URL, Bool) -> Void
    @State private var text = ""
    @State private var tall = false
    @State private var picking = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Address or search") {
                    TextField("xbox.com/play, 100.64.0.5:8080, or words to search", text: $text)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.webSearch)
                        .submitLabel(.go).onSubmit { open(WebAddress.url(for: text)) }
                    Button("Open") { open(WebAddress.url(for: text)) }.disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                Section("Bookmarks") {
                    ForEach(bookmarks) { b in
                        Button { open(URL(string: b.url)) } label: {
                            VStack(alignment: .leading) { Text(b.name); Text(b.url).font(.caption).foregroundStyle(.secondary) }
                        }
                    }
                }
                Section("On this iPad") {
                    Button { picking = true } label: { Label("Open a file… (video, PDF, picture)", systemImage: "folder") }
                }
                Section { Toggle("Tall window (phone-shaped page)", isOn: $tall) }
            }
            .navigationTitle("Open on the glasses")
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
            .fileImporter(isPresented: $picking, allowedContentTypes: [.movie, .audio, .pdf, .image, .html, .plainText]) { result in
                if case let .success(url) = result { open(url) }
            }
        }
    }

    private func open(_ url: URL?) {
        guard let url else { return }
        onPick(url, tall)
        dismiss()
    }
}

/// The focused window's live page on the iPad's own screen: real touches,
/// the on-screen keyboard and the system's password AutoFill (iCloud
/// Keychain, or Chrome / another manager chosen in Settings → Passwords).
/// Sign in here once; the session is kept for every window and every launch.
struct OnIPadSheet: View {
    @Environment(\.dismiss) private var dismiss
    let window: HostWindow

    var body: some View {
        NavigationStack {
            WebHolder(window: window)
                .ignoresSafeArea(edges: .bottom)
                .navigationTitle(window.subtitle)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItemGroup(placement: .topBarLeading) {
                        Button { window.web.goBack() } label: { Image(systemName: "chevron.backward") }
                        Button { window.web.reload() } label: { Image(systemName: "arrow.clockwise") }
                    }
                    ToolbarItem(placement: .confirmationAction) { Button("Back to glasses") { dismiss() } }
                }
        }
        .onDisappear { window.returnToGlasses() }
    }

    private struct WebHolder: UIViewRepresentable {
        let window: HostWindow
        func makeUIView(context: Context) -> UIView {
            let v = UIView()
            v.backgroundColor = .systemBackground
            return v
        }
        func updateUIView(_ v: UIView, context: Context) {
            if window.web.superview !== v { window.moveToIPad(into: v) }
        }
    }
}

/// Sound of the pages: the system volume for wherever the sound is going,
/// and a picker for where that is. With glasses attached iPadOS may send
/// sound to the glasses' own speakers, whose level the iPad cannot change;
/// choose the headphones here and the volume keys work again.
struct SoundRow: View {
    var body: some View {
        HStack(spacing: 8) {
            Text("sound").font(.caption2)
            Image(systemName: "speaker.fill").font(.footnote)
            SystemVolume().frame(height: 28)
            Image(systemName: "speaker.wave.3.fill").font(.footnote)
            RoutePicker().frame(width: 40, height: 40).accessibilityLabel("sound output")
        }
    }

    private struct SystemVolume: UIViewRepresentable {
        func makeUIView(context: Context) -> MPVolumeView { MPVolumeView(frame: .zero) }
        func updateUIView(_ v: MPVolumeView, context: Context) {}
    }

    private struct RoutePicker: UIViewRepresentable {
        func makeUIView(context: Context) -> AVRoutePickerView {
            let v = AVRoutePickerView()
            v.prioritizesVideoDevices = false
            return v
        }
        func updateUIView(_ v: AVRoutePickerView, context: Context) {}
    }
}
