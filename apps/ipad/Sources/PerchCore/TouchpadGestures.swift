import Foundation

/// The trackpad's gesture vocabulary, independent of UIKit so it can be
/// tested: one finger moves the cursor (accelerated), tap = click, two-finger
/// tap = right click, double-tap-and-hold or a stroke from the rim = drag,
/// two fingers = scroll, three-finger swipe = next / previous window.
/// Same behaviour as the Android pop-up (`PhoneTouchpad`).
public final class TouchpadGestures {
    public weak var target: InputTarget?
    /// Every stroke is a drag (the corner lock).
    public var dragLock = false
    /// Strokes that start within this distance of the pad edge are drags.
    public var edge: Float = 0
    /// Pad size, same unit as the touch coordinates.
    public var padW: Float = 0
    public var padH: Float = 0
    /// Cursor gain and acceleration (per unit of finger travel in one event).
    public var gain: Float = 2.2
    public var accelDivisor: Float = 15
    /// Finger travel per wheel line.
    public var scrollStep: Float = 14
    /// Three-finger travel that counts as a swipe.
    public var swipeDistance: Float = 80

    private var lastX: Float = 0, lastY: Float = 0
    private var downT = 0.0, lastTapT = -Double.infinity
    private var fingers = 0, maxFingers = 0
    private var moved = false, dragging = false, swiped = false
    private var scrollAccX: Float = 0, scrollAccY: Float = 0
    private var threeStartX: Float = 0

    public init() {}

    /// A finger went down; `count` fingers are now on the pad and the first
    /// one (the one that is tracked) is at `x`, `y`.
    public func down(count: Int, x: Float, y: Float, t: Double) {
        if fingers == 0 {
            downT = t; maxFingers = 0; moved = false; swiped = false
            scrollAccX = 0; scrollAccY = 0
            let onRim = edge > 0 && (x < edge || y < edge || x > padW - edge || y > padH - edge)
            if dragLock || onRim || t - lastTapT < 0.25 { dragging = true; target?.pressDown() }
        }
        fingers = count
        maxFingers = max(maxFingers, count)
        if count == 3 { threeStartX = x }
        lastX = x; lastY = y
    }

    /// The tracked finger moved to `x`, `y` with `count` fingers down.
    public func move(count: Int, x: Float, y: Float) {
        let dx = x - lastX, dy = y - lastY
        lastX = x; lastY = y
        if abs(dx) + abs(dy) > 1 { moved = true }
        switch count {
        case 1:
            let a = 1 + (abs(dx) + abs(dy)) / accelDivisor
            target?.cursorMove(dx: dx * gain * a, dy: dy * gain * a)
        case 2:
            // Content follows the fingers: fingers down = wheel up.
            scrollAccX += dx; scrollAccY += dy
            let ly = (scrollAccY / scrollStep).rounded(.towardZero)
            let lx = (scrollAccX / scrollStep).rounded(.towardZero)
            if ly != 0 || lx != 0 {
                scrollAccY -= ly * scrollStep; scrollAccX -= lx * scrollStep
                target?.scroll(dx: lx, dy: ly)
            }
        case 3:
            if !swiped, abs(x - threeStartX) > swipeDistance {
                swiped = true
                target?.focusNext(x > threeStartX ? 1 : -1)
            }
        default: break
        }
    }

    /// A finger lifted; `remaining` are still down.
    public func up(remaining: Int, t: Double) {
        fingers = remaining
        guard remaining == 0 else { return }
        if dragging {
            dragging = false
            target?.pressUp()
        } else if !moved && t - downT < 0.22 {
            switch maxFingers {
            case 1: target?.tap(); lastTapT = t
            case 2: target?.rightTap()
            default: break
            }
        }
        maxFingers = 0
    }

    /// The system took the touches away.
    public func cancel() {
        fingers = 0; maxFingers = 0
        if dragging { dragging = false; target?.pressUp() }
    }
}
