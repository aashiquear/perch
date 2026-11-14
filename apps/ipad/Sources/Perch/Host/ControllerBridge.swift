import GameController
import PerchCore

/// A controller paired with the iPad, read through GameController and handed
/// on in the pop-up's own layout (8 axes + button bits), so a real pad and
/// the on-screen skins reach a page the same way. The glasses window cannot
/// be the key window, so WebKit's own controller support may never see it.
@MainActor
final class ControllerBridge {
    private let onState: ([Float], UInt16) -> Void
    private var tokens: [NSObjectProtocol] = []

    init(onState: @escaping ([Float], UInt16) -> Void) {
        self.onState = onState
        tokens.append(NotificationCenter.default.addObserver(forName: .GCControllerDidConnect, object: nil, queue: .main) { [weak self] n in
            let c = n.object as? GCController
            MainActor.assumeIsolated { if let c { self?.attach(c) } }
        })
        GCController.controllers().forEach(attach)
    }

    private func attach(_ c: GCController) {
        guard let g = c.extendedGamepad else { return }
        g.valueChangedHandler = { [weak self] pad, _ in
            let state = Self.read(pad)
            MainActor.assumeIsolated { self?.onState(state.0, state.1) }
        }
    }

    /// Stick Y is up-positive in GameController and down-positive on the wire.
    nonisolated private static func read(_ g: GCExtendedGamepad) -> ([Float], UInt16) {
        let axes: [Float] = [
            g.leftThumbstick.xAxis.value, -g.leftThumbstick.yAxis.value, g.rightThumbstick.xAxis.value, -g.rightThumbstick.yAxis.value,
            g.leftTrigger.value, g.rightTrigger.value,
            g.dpad.right.isPressed ? 1 : (g.dpad.left.isPressed ? -1 : 0), g.dpad.down.isPressed ? 1 : (g.dpad.up.isPressed ? -1 : 0),
        ]
        var bits: UInt16 = 0
        func set(_ b: PadButton, _ on: Bool?) { if on == true { bits |= 1 << UInt16(b.rawValue) } }
        set(.a, g.buttonA.isPressed); set(.b, g.buttonB.isPressed); set(.x, g.buttonX.isPressed); set(.y, g.buttonY.isPressed)
        set(.l1, g.leftShoulder.isPressed); set(.r1, g.rightShoulder.isPressed)
        set(.l2, g.leftTrigger.isPressed); set(.r2, g.rightTrigger.isPressed)
        set(.select, g.buttonOptions?.isPressed); set(.start, g.buttonMenu.isPressed); set(.mode, g.buttonHome?.isPressed)
        set(.thumbL, g.leftThumbstickButton?.isPressed); set(.thumbR, g.rightThumbstickButton?.isPressed)
        return (axes, bits)
    }
}
