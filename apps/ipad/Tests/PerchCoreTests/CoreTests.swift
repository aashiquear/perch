import XCTest
@testable import PerchCore

private final class Recorder: InputTarget {
    var log: [String] = []
    var moveX: Float = 0, moveY: Float = 0
    func cursorMove(dx: Float, dy: Float) { moveX += dx; moveY += dy; log.append("move") }
    func tap() { log.append("tap") }
    func rightTap() { log.append("right") }
    func pressDown() { log.append("down") }
    func pressUp() { log.append("up") }
    func scroll(dx: Float, dy: Float) { log.append("scroll \(Int(dx)) \(Int(dy))") }
    func focusNext(_ step: Int) { log.append("next \(step)") }
}

final class TouchpadGesturesTests: XCTestCase {
    private func pad() -> (TouchpadGestures, Recorder) {
        let g = TouchpadGestures(); let r = Recorder()
        g.target = r; g.padW = 600; g.padH = 400; g.edge = 20
        return (g, r)
    }

    func testTapIsAClick() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.up(remaining: 0, t: 0.1)
        XCTAssertEqual(r.log, ["tap"])
    }

    func testSlowPressIsNotAClick() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.up(remaining: 0, t: 0.5)
        XCTAssertEqual(r.log, [])
    }

    func testMoveIsAcceleratedAndKeepsDirection() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0)
        g.move(count: 1, x: 310, y: 200)
        XCTAssertGreaterThan(r.moveX, 10 * g.gain)   // faster than linear
        XCTAssertEqual(r.moveY, 0)
        g.up(remaining: 0, t: 0.1)
        XCTAssertEqual(r.log, ["move"])              // a moved stroke is no tap
    }

    func testTwoFingerTapIsRightClick() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.down(count: 2, x: 300, y: 200, t: 0.02)
        g.up(remaining: 1, t: 0.08); g.up(remaining: 0, t: 0.1)
        XCTAssertEqual(r.log, ["right"])
    }

    func testTwoFingerScrollFollowsFingers() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.down(count: 2, x: 300, y: 200, t: 0.02)
        g.move(count: 2, x: 300, y: 200 + g.scrollStep * 2 + 1)   // fingers down
        XCTAssertEqual(r.log, ["scroll 0 2"])                     // wheel up
        g.move(count: 2, x: 300 - g.scrollStep - 1, y: 200 + g.scrollStep * 2 + 1)
        XCTAssertEqual(r.log.last, "scroll -1 0")
        g.up(remaining: 1, t: 1); g.up(remaining: 0, t: 1)
        XCTAssertFalse(r.log.contains("right"))
    }

    func testDoubleTapHoldDrags() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.up(remaining: 0, t: 0.05)
        g.down(count: 1, x: 300, y: 200, t: 0.15)
        g.move(count: 1, x: 320, y: 200)
        g.up(remaining: 0, t: 0.6)
        XCTAssertEqual(r.log, ["tap", "down", "move", "up"])
    }

    func testRimStrokeAndLockDrag() {
        let (g, r) = pad()
        g.down(count: 1, x: 5, y: 200, t: 0); g.up(remaining: 0, t: 0.4)
        XCTAssertEqual(r.log, ["down", "up"])
        r.log = []; g.dragLock = true
        g.down(count: 1, x: 300, y: 200, t: 2); g.up(remaining: 0, t: 2.4)
        XCTAssertEqual(r.log, ["down", "up"])
    }

    func testCancelReleasesTheButton() {
        let (g, r) = pad()
        g.dragLock = true
        g.down(count: 1, x: 300, y: 200, t: 0); g.cancel()
        XCTAssertEqual(r.log, ["down", "up"])
    }

    func testThreeFingerSwipeSwitchesWindowOnce() {
        let (g, r) = pad()
        g.down(count: 1, x: 300, y: 200, t: 0); g.down(count: 2, x: 300, y: 200, t: 0); g.down(count: 3, x: 300, y: 200, t: 0)
        g.move(count: 3, x: 400, y: 200); g.move(count: 3, x: 500, y: 200)
        g.up(remaining: 0, t: 0.3)
        XCTAssertEqual(r.log, ["next 1"])
        r.log = []
        g.down(count: 3, x: 300, y: 200, t: 1); g.move(count: 3, x: 200, y: 200); g.up(remaining: 0, t: 1.3)
        XCTAssertEqual(r.log, ["next -1"])
    }
}

final class RemoteInputTests: XCTestCase {
    private func remote() -> (RemoteInput, () -> [Postcard.Event]) {
        var sent: [Postcard.Event] = []
        let r = RemoteInput { sent += $0 }
        return (r, { defer { sent = [] }; return sent })
    }

    func testClicks() {
        let (r, take) = remote()
        r.tap()
        XCTAssertEqual(take(), [.mouseButton(button: 0, down: true), .mouseButton(button: 0, down: false)])
        r.rightTap()
        XCTAssertEqual(take(), [.mouseButton(button: 1, down: true), .mouseButton(button: 1, down: false)])
    }

    func testTypingSendsOnlyTheDifference() {
        let (r, take) = remote()
        r.typed(old: "he", new: "hello")
        XCTAssertEqual(take(), [.text("l"), .text("l"), .text("o")])
        r.typed(old: "hello", new: "hel")
        XCTAssertEqual(take(), [
            .key(usage: 0x2a, down: true, mods: 0), .key(usage: 0x2a, down: false, mods: 0),
            .key(usage: 0x2a, down: true, mods: 0), .key(usage: 0x2a, down: false, mods: 0),
        ])
        r.typed(old: "teh", new: "the")      // autocorrect rewrite: not replayed
        XCTAssertEqual(take(), [])
    }

    func testButtonsUseTheWireBits() {
        let (r, take) = remote()
        r.button(.a, down: true); r.button(.start, down: true)
        XCTAssertEqual(r.buttons, 0x0801)
        r.button(.a, down: false)
        XCTAssertEqual(r.buttons, 0x0800)
        XCTAssertEqual(take().count, 3)
        r.button(.thumbR, down: true)
        XCTAssertEqual(r.buttons, 0x4800)
    }

    func testTriggerButtonAlsoDrivesItsAxis() {
        let (r, _) = remote()
        r.button(.r2, down: true)
        XCTAssertEqual(r.axes[5], 1)
        XCTAssertEqual(r.buttons, 1 << 9)
        r.button(.r2, down: false)
        XCTAssertEqual(r.axes[5], 0)
    }

    func testAxesAreClampedAndRateLimited() {
        let (r, take) = remote()
        r.setAxes([(0, 2), (4, -1)], force: false, now: 1.0)
        XCTAssertEqual(r.axes[0], 1); XCTAssertEqual(r.axes[4], 0)
        XCTAssertEqual(take().count, 1)
        r.setAxes([(0, 0.5)], force: false, now: 1.005)     // too soon: stored, not sent
        XCTAssertEqual(take().count, 0)
        XCTAssertEqual(r.axes[0], 0.5)
        r.setAxes([(0, 0)], force: true, now: 1.006)        // release is always sent
        XCTAssertEqual(take(), [.gamepad(axes: [0, 0, 0, 0, 0, 0, 0, 0], buttons: 0)])
    }

    func testAnnounceResets() {
        let (r, take) = remote()
        r.button(.b, down: true); r.hat(x: -1, y: 0); _ = take()
        r.announce()
        XCTAssertEqual(take(), [.gamepad(axes: [0, 0, 0, 0, 0, 0, 0, 0], buttons: 0)])
    }

    func testKeysToCharacters() {
        XCTAssertEqual(KeyMap.character(usage: 0x04, shift: false), "a")
        XCTAssertEqual(KeyMap.character(usage: 0x1d, shift: true), "Z")
        XCTAssertEqual(KeyMap.character(usage: 0x1e, shift: false), "1")
        XCTAssertEqual(KeyMap.character(usage: 0x1f, shift: true), "@")
        XCTAssertEqual(KeyMap.character(usage: 0x27, shift: false), "0")
        XCTAssertEqual(KeyMap.character(usage: 0x2c, shift: false), " ")
        XCTAssertEqual(KeyMap.character(usage: 0x38, shift: true), "?")
        XCTAssertEqual(KeyMap.character(usage: 0x31, shift: false), "\\")
        XCTAssertEqual(KeyMap.character(usage: 0x34, shift: true), "\"")
        XCTAssertNil(KeyMap.character(usage: 0x28, shift: false))   // enter
        XCTAssertNil(KeyMap.character(usage: 0x32, shift: false))
        XCTAssertNil(KeyMap.character(usage: 0xe1, shift: true))    // shift itself
    }

    func testCommandKeyBecomesControl() {
        XCTAssertEqual(KeyMap.mods(ctrl: false, shift: true, alt: false, cmd: true, cmdAsCtrl: true), 3)
        XCTAssertEqual(KeyMap.mods(ctrl: false, shift: false, alt: true, cmd: true, cmdAsCtrl: false), 12)
        XCTAssertEqual(KeyMap.usage(0xe3, cmdAsCtrl: true), 0xe0)
        XCTAssertEqual(KeyMap.usage(0xe7, cmdAsCtrl: true), 0xe4)
        XCTAssertEqual(KeyMap.usage(0xe3, cmdAsCtrl: false), 0xe3)
        XCTAssertEqual(KeyMap.usage(0x04, cmdAsCtrl: true), 0x04)
    }
}

final class HostStateTests: XCTestCase {
    private func json(_ s: String) -> [String: Any] {
        // swiftlint:disable:next force_cast
        try! JSONSerialization.jsonObject(with: Data(s.utf8)) as! [String: Any]
    }

    /// Shape as perchd sends it (crates/perch-proto, `Event::State`).
    private let sample = """
    {"msg":"event","event":"state","peers":[{"name":"laptop"},{"name":"iPad"}],"glasses":"Rokid Max",
     "display_mode":"mono120","calibration":{"calibrating":{"percent":42}},"layouts":["desk"],"focused":"b",
     "grid":{"rows":1,"row_h_m":0.9,"gap_m":0.05,"distance":2.0,"center_yaw":0.0,"center_pitch":0.0,
             "px_per_m":1600.0,"max_w_m":2.4,"block_w_m":0.6},
     "windows":[
      {"id":"a","source":{"kind":"virtual","peer":"p","width":960,"height":1080},
       "placement":{"yaw":-0.5235988,"pitch":0.0,"distance":2.0,"roll":0.0,"width_m":0.8,"height_m":0.9},
       "anchor":"world","z":0,"opacity":1.0,"visible":true,"focused":false},
      {"id":"b","source":{"kind":"window","peer":"p","handle":"1","title":"nvim"},
       "placement":{"yaw":0.5235988,"pitch":0.0,"distance":2.0,"roll":0.0,"width_m":1.6,"height_m":0.9},
       "anchor":"world","z":1,"opacity":1.0,"visible":true,"focused":true},
      {"id":"c","source":{"kind":"display","peer":"p","display_index":1},
       "placement":{"yaw":0.0,"pitch":0.0,"distance":2.0,"roll":0.0,"width_m":1.6,"height_m":0.9},
       "anchor":"world","z":2,"opacity":1.0,"visible":false,"focused":false}]}
    """

    func testParsesState() {
        let (s, grid) = HostState.parse(json(sample))
        XCTAssertEqual(s.glasses, "Rokid Max")
        XCTAssertEqual(s.displayMode, "mono120")
        XCTAssertEqual(s.calibration, .calibrating(42))
        XCTAssertEqual(s.peers, 2)
        XCTAssertEqual(s.layouts, ["desk"])
        XCTAssertEqual(s.gridDistance, 2.0)
        XCTAssertNotNil(grid)
        XCTAssertEqual(s.windows.map(\.label), ["Virtual 960×1080", "nvim", "Display 1"])
        XCTAssertEqual(s.windows.map(\.focused), [false, true, false])
        XCTAssertEqual(s.windows[0].yawDeg, -30, accuracy: 0.01)
        XCTAssertTrue(s.windows[0].portrait)
        XCTAssertFalse(s.windows[1].portrait)
    }

    func testMapOrderIsLeftToRightAndSkipsHidden() {
        let (s, _) = HostState.parse(json(sample))
        XCTAssertEqual(s.mapOrder.map(\.id), ["b", "a"])   // +yaw is left
    }

    func testCalibrationShapes() {
        XCTAssertEqual(Calibration(json: "ready"), .ready)
        XCTAssertEqual(Calibration(json: "uncalibrated"), .uncalibrated)
        XCTAssertEqual(Calibration(json: nil), .uncalibrated)
        XCTAssertEqual(Calibration.calibrating(7).label, "calibrating 7%")
    }

    /// Shapes recorded from perchd (mock, 2026-11-13).
    func testCalibrationEvents() {
        XCTAssertEqual(Calibration(event: json(#"{"msg":"event","event":"calibration","calibrating":{"percent":51}}"#)), .calibrating(51))
        XCTAssertEqual(Calibration(event: json(#"{"msg":"event","event":"calibration","ready":null}"#)), .ready)
        XCTAssertEqual(Calibration(event: json(#"{"msg":"event","event":"calibration","uncalibrated":null}"#)), .uncalibrated)
        XCTAssertNil(Calibration(event: json(#"{"msg":"event","event":"calibration"}"#)))
    }

    func testEmptyStateDoesNotCrash() {
        let (s, grid) = HostState.parse([:])
        XCTAssertEqual(s, HostState())
        XCTAssertNil(grid)
    }

    func testGridDistanceIsClamped() {
        let (_, grid) = HostState.parse(json(sample))
        XCTAssertEqual(HostState.grid(grid, distanceBy: 0.25)?["distance"] as? Double, 2.25)
        XCTAssertEqual(HostState.grid(grid, distanceBy: -5)?["distance"] as? Double, 0.8)
        XCTAssertEqual(HostState.grid(grid, distanceBy: 0.25)?["rows"] as? Int, 1)
        XCTAssertNil(HostState.grid(nil, distanceBy: 1))
    }
}
