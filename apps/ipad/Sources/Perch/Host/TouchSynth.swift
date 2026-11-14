import Darwin
import UIKit

/// A genuine tap on a view of this app, made the way the system makes one: a
/// `UITouch` carried by the application's touch event, sent through
/// `UIApplication.sendEvent`. WebKit then produces *trusted* touch, pointer
/// and click events, which scripted events never are — and which some sites
/// (Xbox Cloud Gaming's "tap anywhere") insist on.
///
/// The glasses display takes no touches, so nothing public can do this. It
/// uses UIKit's internal setters, the same ones UI-test frameworks drive (KIF
/// is the reference). Every selector is checked first; if this iPadOS no
/// longer has one, `tap` reports it and the caller falls back to script.
@MainActor
enum TouchSynth {
    @objc private protocol TouchSetters {
        func setWindow(_ w: UIWindow?)
        func setView(_ v: UIView?)
        func setGestureView(_ v: UIView?)
        func setPhase(_ p: UITouch.Phase)
        func setTapCount(_ n: UInt)
        func setTimestamp(_ t: TimeInterval)
        @objc(_setLocationInWindow:resetPrevious:) func setLocation(_ p: CGPoint, resetPrevious: Bool)
        @objc(_setIsFirstTouchForView:) func setIsFirstTouchForView(_ b: Bool)
        @objc(_setHidEvent:) func setHidEvent(_ e: UnsafeMutableRawPointer?)
    }

    @objc private protocol EventSetters {
        @objc(_clearTouches) func clearTouches()
        @objc(_setHIDEvent:) func setHIDEvent(_ e: UnsafeMutableRawPointer?)
        @objc(_addTouch:forDelayedDelivery:) func addTouch(_ t: UITouch, delayed: Bool)
    }

    @objc private protocol AppEvents {
        @objc(_touchesEvent) func touchesEvent() -> UIEvent?
    }

    // IOHIDEvent (IOKit): the low-level record UIKit and WebKit expect behind a touch.
    private typealias CreateDigitizer = @convention(c) (CFAllocator?, UInt64, UInt32, UInt32, UInt32, UInt32, UInt32, Double, Double, Double, Double, Double, Bool, Bool, UInt32) -> UnsafeMutableRawPointer?
    private typealias CreateFinger = @convention(c) (CFAllocator?, UInt64, UInt32, UInt32, UInt32, Double, Double, Double, Double, Double, Bool, Bool, UInt32) -> UnsafeMutableRawPointer?
    private typealias SetInteger = @convention(c) (UnsafeMutableRawPointer, UInt32, Int32, UInt32) -> Void
    private typealias Append = @convention(c) (UnsafeMutableRawPointer, UnsafeMutableRawPointer, UInt32) -> Void

    private static func sym<T>(_ name: String, as _: T.Type) -> T? {
        guard let p = dlsym(UnsafeMutableRawPointer(bitPattern: -2), name) else { return nil }   // RTLD_DEFAULT
        return unsafeBitCast(p, to: T.self)
    }

    private static let touchSelectors = ["setWindow:", "setView:", "setPhase:", "setTapCount:", "setTimestamp:",
                                         "_setLocationInWindow:resetPrevious:", "_setIsFirstTouchForView:", "_setHidEvent:"]
    private static let eventSelectors = ["_clearTouches", "_setHIDEvent:", "_addTouch:forDelayedDelivery:"]

    /// What is missing on this system, or nil if a tap can be made.
    static let unavailable: String? = {
        _ = dlopen("/System/Library/Frameworks/IOKit.framework/IOKit", RTLD_NOW)
        for s in touchSelectors where !UITouch.instancesRespond(to: NSSelectorFromString(s)) { return "UITouch lacks \(s)" }
        guard UIApplication.shared.responds(to: NSSelectorFromString("_touchesEvent")) else { return "no _touchesEvent" }
        for s in eventSelectors where !UIEvent.instancesRespond(to: NSSelectorFromString(s)) {
            // The touches event is a subclass; ask the live object as well.
            let live = unsafeBitCast(UIApplication.shared, to: AppEvents.self).touchesEvent()
            if live?.responds(to: NSSelectorFromString(s)) != true { return "touch event lacks \(s)" }
        }
        for f in ["IOHIDEventCreateDigitizerEvent", "IOHIDEventCreateDigitizerFingerEvent", "IOHIDEventSetIntegerValueWithOptions", "IOHIDEventAppendEvent"]
        where dlsym(UnsafeMutableRawPointer(bitPattern: -2), f) == nil { return "no \(f)" }
        return nil
    }()

    /// The HID record for one finger at `p`, touching or lifted. Caller releases.
    private static func hidEvent(at p: CGPoint, touching: Bool) -> UnsafeMutableRawPointer? {
        guard let hand = sym("IOHIDEventCreateDigitizerEvent", as: CreateDigitizer.self),
              let finger = sym("IOHIDEventCreateDigitizerFingerEvent", as: CreateFinger.self),
              let setInt = sym("IOHIDEventSetIntegerValueWithOptions", as: SetInteger.self),
              let append = sym("IOHIDEventAppendEvent", as: Append.self) else { return nil }
        let now = mach_absolute_time()
        let displayIntegrated: UInt32 = 0xB0019          // digitizer field 25
        let rangeAndTouch: UInt32 = 0x1 | 0x2
        // transducer 3 = hand, event mask 2 = touch
        guard let h = hand(kCFAllocatorDefault, now, 3, 0, 0, 2, 0, 0, 0, 0, 0, 0, false, true, 0) else { return nil }
        setInt(h, displayIntegrated, 1, 0)
        if let f = finger(kCFAllocatorDefault, now, 1, 2, rangeAndTouch, Double(p.x), Double(p.y), 0, 0, 0, touching, touching, 0) {
            setInt(f, displayIntegrated, 1, 0)
            append(h, f, 0)
            Unmanaged<AnyObject>.fromOpaque(f).release()
        }
        return h
    }

    private static func send(_ touch: UITouch, at p: CGPoint, touching: Bool) {
        guard let event = unsafeBitCast(UIApplication.shared, to: AppEvents.self).touchesEvent() else { return }
        let hid = hidEvent(at: p, touching: touching)
        unsafeBitCast(touch, to: TouchSetters.self).setHidEvent(hid)
        let e = unsafeBitCast(event, to: EventSetters.self)
        e.clearTouches()
        e.setHIDEvent(hid)
        e.addTouch(touch, delayed: false)
        UIApplication.shared.sendEvent(event)
        if let hid { Unmanaged<AnyObject>.fromOpaque(hid).release() }
    }

    /// Tap at `point` (window coordinates) if the view there belongs to
    /// `expected`. Returns nil on success, otherwise why not.
    static func tap(at point: CGPoint, in window: UIWindow, expected: UIView) -> String? {
        if let unavailable { return unavailable }
        guard let hit = window.hitTest(point, with: nil) else { return "nothing at the point" }
        guard hit.isDescendant(of: expected) else { return "the point hits \(type(of: hit)), not the page" }
        let touch = UITouch()
        let t = unsafeBitCast(touch, to: TouchSetters.self)
        t.setWindow(window)
        t.setLocation(point, resetPrevious: true)
        t.setView(hit)
        if touch.responds(to: NSSelectorFromString("setGestureView:")) { t.setGestureView(hit) }
        t.setPhase(.began)
        t.setTapCount(1)
        t.setIsFirstTouchForView(true)
        t.setTimestamp(ProcessInfo.processInfo.systemUptime)
        send(touch, at: point, touching: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.06) {
            t.setLocation(point, resetPrevious: false)
            t.setPhase(.ended)
            t.setTimestamp(ProcessInfo.processInfo.systemUptime)
            send(touch, at: point, touching: false)
        }
        return nil
    }
}
