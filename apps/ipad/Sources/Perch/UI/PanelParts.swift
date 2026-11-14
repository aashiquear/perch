import PerchCore
import SwiftUI

// The panel's building blocks, each the twin of a composable in the Android
// `PanelScreen.kt` (Stat, Action, Small, LayoutMap, PresetPicture).

/// A number with a caption under it.
struct Stat: View {
    @Environment(\.palette) private var p
    let value: String
    let caption: String

    var body: some View {
        VStack(spacing: 2) {
            Text(value).font(.title2.bold()).lineLimit(1).minimumScaleFactor(0.6)
            Text(caption).font(.caption2)
        }
        .frame(maxWidth: .infinity).padding(.vertical, 8).padding(.horizontal, 4)
        .background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
    }
}

/// Square icon tile with the name and a hint beside it (the Linux panel's
/// layout). Use it as the label of a `Button` or a `Menu`.
struct ActionLabel: View {
    @Environment(\.palette) private var p
    let icon: String
    let label: String
    let hint: String
    var selected = false

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon).font(.system(size: 24))
                .frame(width: 56, height: 56)
                .foregroundStyle(selected ? p.onPrimary : p.onSurface)
                .background(selected ? p.primary : p.surfaceVariant, in: RoundedRectangle(cornerRadius: 12))
            VStack(alignment: .leading, spacing: 1) {
                Text(label).font(.body).foregroundStyle(p.onSurface)
                Text(hint).font(.caption2).foregroundStyle(p.onSurfaceVariant)
            }
            .multilineTextAlignment(.leading)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// `ActionLabel` as a button.
struct Action: View {
    let icon: String
    let label: String
    let hint: String
    var selected = false
    let action: () -> Void

    var body: some View {
        Button(action: action) { ActionLabel(icon: icon, label: label, hint: hint, selected: selected) }
            .buttonStyle(.plain)
    }
}

/// Compact round icon button with a one-word caption under it.
struct Small: View {
    @Environment(\.palette) private var p
    let icon: String
    let label: String
    var selected = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 2) {
                Image(systemName: icon).font(.system(size: 17))
                    .frame(width: 40, height: 40)
                    .foregroundStyle(selected ? p.onPrimary : p.onSurface)
                    .background(selected ? p.primary : p.surfaceVariant, in: Circle())
                Text(label).font(.caption2).lineLimit(1).fixedSize()
            }
            .frame(width: 68)
        }
        .buttonStyle(.plain)
    }
}

/// A chip that is filled while selected (Material's FilterChip).
struct Chip: View {
    @Environment(\.palette) private var p
    let label: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) { ChipLabel(label: label, selected: selected) }.buttonStyle(.plain)
    }
}

struct ChipLabel: View {
    @Environment(\.palette) private var p
    let label: String
    let selected: Bool

    var body: some View {
        Text(label).font(.subheadline.weight(.medium)).lineLimit(1)
            .padding(.horizontal, 12).padding(.vertical, 7)
            .foregroundStyle(p.onSurface)
            .background(selected ? p.secondaryContainer : Color.clear, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.onSurfaceVariant.opacity(selected ? 0 : 0.5), lineWidth: 1))
            .contentShape(Rectangle())
    }
}

/// The windows as boxes, left to right as they hang in front of you; the
/// focused one bright. Tap a box to focus it.
struct LayoutMap: View {
    let windows: [WindowInfo]
    let onTap: (String) -> Void

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x12141A))
            if windows.isEmpty {
                Text("no windows on the glasses").font(.footnote).foregroundStyle(Color(hex: 0x6E7380))
            }
            HStack(spacing: 6) {
                ForEach(Array(windows.enumerated()), id: \.element.id) { i, w in
                    let h: CGFloat = w.portrait ? 68 : 40
                    let aspect = CGFloat(w.heightM > 0 ? w.widthM / w.heightM : 1.6)
                    Text("\(i + 1)").font(.caption.weight(.medium)).foregroundStyle(.white)
                        .frame(width: min(max(h * aspect, 24), 110), height: h)
                        .background(Color(hex: w.focused ? 0x468CC8 : 0x3C465A), in: RoundedRectangle(cornerRadius: 4))
                        .onTapGesture { onTap(w.id) }
                }
            }
        }
        .frame(maxWidth: .infinity).frame(height: 90)
    }
}

/// A preset as a picture: boxes with the given width / height ratios.
struct PresetPicture: View {
    @Environment(\.palette) private var p
    let name: String
    let ratios: [CGFloat]
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 2) {
                HStack(spacing: 3) {
                    ForEach(Array(ratios.enumerated()), id: \.offset) { _, r in
                        RoundedRectangle(cornerRadius: 3).fill(Color(hex: 0x7FA7D9)).frame(width: 16 * r, height: 16)
                    }
                }
                .frame(width: 96, height: 48)
                .background(p.surfaceVariant, in: RoundedRectangle(cornerRadius: 10))
                Text(name).font(.caption2).foregroundStyle(p.onSurface)
            }
        }
        .buttonStyle(.plain)
    }
}

/// Children laid out left to right, wrapping to the next line, each line
/// spread evenly (Compose's FlowRow with SpaceEvenly).
struct FlowRow: Layout {
    var spacing: CGFloat = 6
    var lineSpacing: CGFloat = 6

    private func lines(_ subviews: Subviews, width: CGFloat) -> [[(Int, CGSize)]] {
        var out: [[(Int, CGSize)]] = [[]]
        var x: CGFloat = 0
        for (i, v) in subviews.enumerated() {
            let s = v.sizeThatFits(.unspecified)
            if x + s.width > width, !out[out.count - 1].isEmpty { out.append([]); x = 0 }
            out[out.count - 1].append((i, s))
            x += s.width + spacing
        }
        return out
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let ls = lines(subviews, width: width)
        let h = ls.reduce(CGFloat(0)) { $0 + ($1.map(\.1.height).max() ?? 0) } + lineSpacing * CGFloat(max(ls.count - 1, 0))
        let natural = ls.map { $0.reduce(CGFloat(0)) { $0 + $1.1.width + spacing } - spacing }.max() ?? 0
        return CGSize(width: width.isFinite ? width : max(natural, 0), height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for line in lines(subviews, width: bounds.width) {
            let used = line.reduce(CGFloat(0)) { $0 + $1.1.width }
            let gap = max((bounds.width - used) / CGFloat(line.count + 1), 0)
            var x = bounds.minX + gap
            let lineH = line.map(\.1.height).max() ?? 0
            for (i, s) in line {
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
                x += s.width + gap
            }
            y += lineH + lineSpacing
        }
    }
}
