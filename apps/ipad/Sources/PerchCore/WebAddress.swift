import Foundation

/// What the address bar does with what was typed.
public enum WebAddress {
    /// A URL for typed text: an address if it looks like one, otherwise a web
    /// search. Hosts on the local network or a tailnet are usually plain
    /// HTTP on a port, so: an IP address, a host with a port, `localhost` and
    /// `.local` names get `http://`; everything else `https://`. Type the
    /// scheme to override.
    public static func url(for input: String) -> URL? {
        let t = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return nil }
        if t.contains("://") { return URL(string: t) ?? search(t) }
        if t.contains(" ") { return search(t) }
        let hostPort = t.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? t
        let parts = hostPort.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        let host = parts.first ?? ""
        let hasPort = parts.count == 2 && !parts[1].isEmpty && parts[1].allSatisfy(\.isNumber)
        guard !host.isEmpty, parts.count <= 2, parts.count == 1 || hasPort else { return search(t) }
        let ip = isIPv4(host)
        let local = host == "localhost" || host.hasSuffix(".local")
        // One bare word is a search unless something marks it as a machine name.
        guard host.contains(".") || hasPort || local else { return search(t) }
        return URL(string: ((ip || hasPort || local) ? "http://" : "https://") + t) ?? search(t)
    }

    static func isIPv4(_ s: String) -> Bool {
        let p = s.split(separator: ".", omittingEmptySubsequences: false)
        return p.count == 4 && p.allSatisfy { !$0.isEmpty && $0.count <= 3 && $0.allSatisfy(\.isNumber) && Int($0)! <= 255 }
    }

    static func search(_ words: String) -> URL? {
        var c = URLComponents(string: "https://www.google.com/search")
        c?.queryItems = [URLQueryItem(name: "q", value: words)]
        return c?.url
    }
}

/// A saved site on the bookmark bar.
public struct Bookmark: Codable, Equatable, Identifiable {
    public var id: String { url }
    public var name: String
    public var url: String

    public init(name: String, url: String) { self.name = name; self.url = url }
}

/// The bookmark bar: ordered, no address twice.
public struct Bookmarks: Codable, Equatable {
    public private(set) var items: [Bookmark]

    public init(_ items: [Bookmark] = Bookmarks.starter) { self.items = items }

    /// What a new install starts with.
    public static let starter = [
        Bookmark(name: "Xbox Cloud", url: "https://www.xbox.com/play"),
        Bookmark(name: "GeForce NOW", url: "https://play.geforcenow.com"),
        Bookmark(name: "YouTube", url: "https://www.youtube.com"),
        Bookmark(name: "Google", url: "https://www.google.com"),
    ]

    /// Add a page (or rename it if its address is already there). The name
    /// falls back to the host.
    public mutating func add(name: String, url: String) {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        let label = trimmed.isEmpty ? (URL(string: url)?.host ?? url) : String(trimmed.prefix(28))
        if let i = items.firstIndex(where: { $0.url == url }) { items[i].name = label } else { items.append(Bookmark(name: label, url: url)) }
    }

    public mutating func remove(url: String) { items.removeAll { $0.url == url } }
    public func contains(url: String) -> Bool { items.contains { $0.url == url } }

    public var json: String { (try? JSONEncoder().encode(items)).flatMap { String(data: $0, encoding: .utf8) } ?? "[]" }
    public init?(json: String) {
        guard let d = json.data(using: .utf8), let v = try? JSONDecoder().decode([Bookmark].self, from: d) else { return nil }
        items = v
    }
}

/// The W3C "standard" gamepad a web page expects, from the controller state
/// the pop-up keeps (`RemoteInput`: 8 axes + button bits).
public enum StandardGamepad {
    /// `axes`: left x, left y, right x, right y. `buttons`: 17 values 0…1 in
    /// the standard order (A B X Y LB RB LT RT Back Start L3 R3 Up Down Left
    /// Right Home).
    public static func map(axes a: [Float], buttons bits: UInt16) -> (axes: [Float], buttons: [Float]) {
        func ax(_ i: Int) -> Float { i < a.count ? a[i] : 0 }
        func on(_ b: PadButton) -> Float { bits & (1 << UInt16(b.rawValue)) != 0 ? 1 : 0 }
        // Triggers are analogue; a pressed trigger button with no axis value still counts.
        let lt = max(ax(4), on(.l2)), rt = max(ax(5), on(.r2))
        let buttons: [Float] = [
            on(.a), on(.b), on(.x), on(.y), on(.l1), on(.r1), lt, rt, on(.select), on(.start), on(.thumbL), on(.thumbR),
            ax(7) < -0.5 ? 1 : 0, ax(7) > 0.5 ? 1 : 0, ax(6) < -0.5 ? 1 : 0, ax(6) > 0.5 ? 1 : 0, on(.mode),
        ]
        return ([ax(0), ax(1), ax(2), ax(3)], buttons)
    }
}
