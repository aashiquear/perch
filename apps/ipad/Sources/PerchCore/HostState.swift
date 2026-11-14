import Foundation

/// Tracker calibration as the host reports it:
/// `"uncalibrated"` | `{"calibrating":{"percent":42}}` | `"ready"`.
public enum Calibration: Equatable {
    case uncalibrated
    case calibrating(Int)
    case ready

    public init(json: Any?) {
        if let s = json as? String {
            self = s == "ready" ? .ready : .uncalibrated
        } else if let o = json as? [String: Any], let c = o["calibrating"] as? [String: Any] {
            self = .calibrating((c["percent"] as? NSNumber)?.intValue ?? 0)
        } else {
            self = .uncalibrated
        }
    }

    /// From a `calibration` event, where the state is flattened into the
    /// event object: `{"calibrating":{"percent":42}}`, `{"ready":null}`,
    /// `{"uncalibrated":null}`. Nil when the object carries none of them.
    public init?(event o: [String: Any]) {
        if let c = o["calibrating"] as? [String: Any] {
            self = .calibrating((c["percent"] as? NSNumber)?.intValue ?? 0)
        } else if o["ready"] != nil {
            self = .ready
        } else if o["uncalibrated"] != nil {
            self = .uncalibrated
        } else {
            return nil
        }
    }

    public var label: String {
        switch self {
        case .uncalibrated: return "not calibrated"
        case let .calibrating(p): return "calibrating \(p)%"
        case .ready: return "calibrated"
        }
    }
}

/// One spatial window of the host, in the units a person reads (degrees).
public struct WindowInfo: Identifiable, Equatable {
    public let id: String
    public let label: String
    /// `display`, `window`, `virtual`, `dex`, `phone_screen`.
    public let kind: String
    public var focused: Bool
    public let visible: Bool
    /// + = left of centre (the project's yaw convention).
    public let yawDeg: Double
    public let pitchDeg: Double
    public let widthM: Double
    public let heightM: Double
    public let distance: Double

    public init(id: String, label: String, kind: String, focused: Bool, visible: Bool, yawDeg: Double, pitchDeg: Double,
                widthM: Double, heightM: Double, distance: Double) {
        self.id = id; self.label = label; self.kind = kind; self.focused = focused; self.visible = visible
        self.yawDeg = yawDeg; self.pitchDeg = pitchDeg; self.widthM = widthM; self.heightM = heightM; self.distance = distance
    }

    /// Taller than wide.
    public var portrait: Bool { heightM > widthM }
}

/// The host's `state` event, reduced to what the panel shows.
public struct HostState: Equatable {
    public var glasses: String?
    public var displayMode: String?
    public var calibration = Calibration.uncalibrated
    public var windows: [WindowInfo] = []
    public var layouts: [String] = []
    public var peers = 0
    /// Distance of the layout grid, metres (nil on hosts older than 1.2).
    public var gridDistance: Double?

    public init() {}

    /// Parse the JSON object of a `state` event. Unknown or missing fields
    /// fall back to defaults; a newer host must not break an older app.
    public static func parse(_ o: [String: Any]) -> (state: HostState, grid: [String: Any]?) {
        var s = HostState()
        s.glasses = o["glasses"] as? String
        s.displayMode = o["display_mode"] as? String
        s.calibration = Calibration(json: o["calibration"])
        s.layouts = o["layouts"] as? [String] ?? []
        s.peers = (o["peers"] as? [Any])?.count ?? 0
        let grid = o["grid"] as? [String: Any]
        s.gridDistance = (grid?["distance"] as? NSNumber)?.doubleValue
        let focused = o["focused"] as? String
        s.windows = (o["windows"] as? [[String: Any]] ?? []).compactMap { window($0, focused: focused) }
        return (s, grid)
    }

    static func window(_ w: [String: Any], focused: String?) -> WindowInfo? {
        guard let id = w["id"] as? String else { return nil }
        let src = w["source"] as? [String: Any] ?? [:]
        let p = w["placement"] as? [String: Any] ?? [:]
        func num(_ k: String) -> Double { (p[k] as? NSNumber)?.doubleValue ?? 0 }
        let kind = src["kind"] as? String ?? "?"
        let label: String
        switch kind {
        case "display": label = "Display \((src["display_index"] as? NSNumber)?.intValue ?? 0)"
        case "window": label = (src["title"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "Window"
        case "virtual":
            let w = (src["width"] as? NSNumber)?.intValue ?? 0, h = (src["height"] as? NSNumber)?.intValue ?? 0
            label = "Virtual \(w)×\(h)"
        case "dex": label = "DeX"
        case "phone_screen": label = "Phone"
        default: label = "?"
        }
        return WindowInfo(
            id: id, label: label, kind: kind, focused: id == focused,
            visible: w["visible"] as? Bool ?? true,
            yawDeg: num("yaw") * 180 / .pi, pitchDeg: num("pitch") * 180 / .pi,
            widthM: num("width_m"), heightM: num("height_m"), distance: num("distance")
        )
    }

    /// Visible windows in the order the eye meets them: left to right (yaw
    /// descending, since + is left), upper row first.
    public var mapOrder: [WindowInfo] {
        windows.filter(\.visible).sorted { a, b in
            if abs(a.pitchDeg - b.pitchDeg) > 8 { return a.pitchDeg > b.pitchDeg }
            return a.yawDeg > b.yawDeg
        }
    }

    /// The grid with its distance moved by `delta` metres, clamped to a
    /// comfortable range; nil when the host sent no grid.
    public static func grid(_ grid: [String: Any]?, distanceBy delta: Double) -> [String: Any]? {
        guard var g = grid, let d = (g["distance"] as? NSNumber)?.doubleValue else { return nil }
        g["distance"] = min(max(d + delta, 0.8), 6.0)
        return g
    }
}
