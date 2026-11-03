import Foundation

/// Minimal postcard encoder for `perch_proto::StreamPacket`.
/// Byte-exact against `wire_format_tests` in crates/perch-proto/src/lib.rs.
enum Postcard {
    enum Event {
        case mouseMove(dx: Float, dy: Float)
        case mouseButton(button: Int, down: Bool)
        case scroll(dx: Float, dy: Float)
        case key(usage: Int, down: Bool, mods: Int)
        case text(String)
        /// Whole controller state (since 1.6).
        case gamepad(axes: [Float], buttons: UInt16)
    }

    static let btnLeft = 0, btnRight = 1, btnMiddle = 2, btnBack = 3, btnForward = 4

    static func ping(_ nonce: UInt32) -> Data {
        var d = Data()
        varint(&d, 2)
        varint(&d, nonce)
        return d
    }

    static func input(_ events: [Event]) -> Data {
        var d = Data()
        varint(&d, 1)
        varint(&d, UInt32(events.count))
        for e in events {
            switch e {
            case let .mouseMove(dx, dy): varint(&d, 0); f32(&d, dx); f32(&d, dy)
            case let .mouseButton(b, down): varint(&d, 2); varint(&d, UInt32(b)); d.append(down ? 1 : 0)
            case let .scroll(dx, dy): varint(&d, 3); f32(&d, dx); f32(&d, dy)
            case let .key(u, down, mods): varint(&d, 4); varint(&d, UInt32(u)); d.append(down ? 1 : 0); d.append(UInt8(mods & 0xff))
            case let .gamepad(axes, buttons):
                varint(&d, 6)
                for i in 0..<8 { f32(&d, i < axes.count ? axes[i] : 0) }
                varint(&d, UInt32(buttons & 0x7fff))
            case let .text(s):
                varint(&d, 5)
                let b = Array(s.utf8)
                varint(&d, UInt32(b.count))
                d.append(contentsOf: b)
            }
        }
        return d
    }

    private static func varint(_ d: inout Data, _ v: UInt32) {
        var x = v
        while x >= 0x80 { d.append(UInt8(x & 0x7f) | 0x80); x >>= 7 }
        d.append(UInt8(x))
    }

    private static func f32(_ d: inout Data, _ f: Float) {
        var le = f.bitPattern.littleEndian
        withUnsafeBytes(of: &le) { d.append(contentsOf: $0) }
    }
}
