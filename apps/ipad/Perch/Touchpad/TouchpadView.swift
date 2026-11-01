import SwiftUI
import UIKit

/// Trackpad surface. Gesture vocabulary matches the Android touchpad and a
/// MacBook trackpad: 1-finger move/tap, 2-finger scroll/right-click,
/// 3-finger swipe to switch window. Pencil hover (iPad Pro) moves the
/// pointer without touching. Batches are flushed every ~8 ms.
struct TouchpadView: UIViewRepresentable {
    let client: PerchClient

    func makeUIView(context: Context) -> TrackpadUIView {
        let v = TrackpadUIView()
        v.onBatch = { client.sendInput($0) }
        v.onThreeSwipe = { _ in client.focusNext() }
        return v
    }
    func updateUIView(_ uiView: TrackpadUIView, context: Context) {}
}

final class TrackpadUIView: UIView {
    var onBatch: (([Postcard.Event]) -> Void)?
    var onThreeSwipe: ((Int) -> Void)?
    var sensitivity: Float = 1.4

    private var batch: [Postcard.Event] = []
    private var lastFlush = CACurrentMediaTime()
    private var last: CGPoint = .zero
    private var start: CGPoint = .zero
    private var downT = CACurrentMediaTime()
    private var maxFingers = 0
    private var moved = false
    private var lastTap = 0.0
    private var dragging = false
    private var scrollAcc: CGFloat = 0

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        backgroundColor = UIColor.secondarySystemBackground
        layer.cornerRadius = 24
        layer.cornerCurve = .continuous
        // iPadOS pointer (trackpad/mouse attached to the iPad) also drives it.
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(onHover(_:)))
        addGestureRecognizer(hover)
    }
    required init?(coder: NSCoder) { fatalError() }

    @objc private func onHover(_ g: UIHoverGestureRecognizer) {
        let p = g.location(in: self)
        if g.state == .changed {
            batch.append(.mouseMove(dx: Float(p.x - last.x) * sensitivity, dy: Float(p.y - last.y) * sensitivity))
            flush(force: false)
        }
        last = p
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        let all = event?.allTouches ?? touches
        maxFingers = max(maxFingers, all.count)
        guard let t = touches.first else { return }
        let p = t.location(in: self)
        if all.count == 1 {
            last = p; start = p; downT = CACurrentMediaTime(); moved = false; scrollAcc = 0
            if downT - lastTap < 0.25 { dragging = true; batch.append(.mouseButton(button: Postcard.btnLeft, down: true)) }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        let all = event?.allTouches ?? touches
        guard let t = all.first else { return }
        let p = t.location(in: self)
        let dx = p.x - last.x, dy = p.y - last.y
        last = p
        if abs(p.x - start.x) > 8 || abs(p.y - start.y) > 8 { moved = true }
        switch all.count {
        case 1: batch.append(.mouseMove(dx: accel(Float(dx)), dy: accel(Float(dy))))
        case 2:
            scrollAcc += dy
            let lines = Int(scrollAcc / 24)
            if lines != 0 { scrollAcc -= CGFloat(lines) * 24; batch.append(.scroll(dx: 0, dy: Float(lines))) }
        case 3:
            if abs(p.x - start.x) > 120 { onThreeSwipe?(p.x > start.x ? 1 : -1); start = p }
        default: break
        }
        flush(force: false)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        let remaining = (event?.allTouches ?? []).filter { $0.phase != .ended && $0.phase != .cancelled }.count
        if remaining == 0 {
            let dt = CACurrentMediaTime() - downT
            if dragging { dragging = false; batch.append(.mouseButton(button: Postcard.btnLeft, down: false)) }
            else if !moved && dt < 0.2 {
                let b = maxFingers >= 3 ? Postcard.btnMiddle : (maxFingers == 2 ? Postcard.btnRight : Postcard.btnLeft)
                batch.append(.mouseButton(button: b, down: true)); batch.append(.mouseButton(button: b, down: false))
                lastTap = CACurrentMediaTime()
            }
            maxFingers = 0
            flush(force: true)
        }
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        if dragging { dragging = false; batch.append(.mouseButton(button: Postcard.btnLeft, down: false)); flush(force: true) }
    }

    private func accel(_ d: Float) -> Float {
        let a = abs(d)
        let gain: Float = a > 20 ? 2.2 : (a > 6 ? 1.5 : 1.0)
        return d * sensitivity * gain
    }

    private func flush(force: Bool) {
        guard !batch.isEmpty else { return }
        let now = CACurrentMediaTime()
        if force || now - lastFlush >= 0.008 { onBatch?(batch); batch.removeAll(keepingCapacity: true); lastFlush = now }
    }
}
