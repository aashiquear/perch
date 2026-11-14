import Foundation

/// The "Bose AR" Bluetooth LE sensor interface (Frames, QC35 II, Noise
/// Cancelling Headphones 700). Bose withdrew its SDK; the layout here follows
/// the community description (zakaton/Bose-Frames-Web-SDK) and is **unverified
/// on our hardware** until the in-app check confirms it. All multi-byte
/// integers are big-endian.
public enum BoseAR {
    public static let service = "FDD2"
    public static let deviceInfo = "7B61AD83-041C-4333-A0AB-EFB2AB7BDD43"
    public static let sensorInfo = "855CB3E7-98FF-42A6-80FC-40B32A2221C1"
    public static let sensorConfig = "5AF38AF6-000E-404B-9B46-07F77580890B"
    public static let sensorData = "56A72AB8-4988-4CC8-A752-FBD1D54A953D"

    /// Sensor ids on the wire.
    public enum Sensor: Int, CaseIterable {
        case accelerometer = 0, gyroscope, rotation, gameRotation, orientation, magnetometer, uncalibratedMagnetometer

        /// Payload bytes after the 3-byte record header.
        public var payloadLength: Int {
            switch self {
            case .accelerometer, .gyroscope, .orientation, .magnetometer: return 7   // vector + accuracy
            case .rotation: return 10                                               // quaternion + accuracy
            case .gameRotation: return 8                                            // quaternion
            case .uncalibratedMagnetometer: return 12                               // vector + bias
            }
        }
    }

    /// One record of a sensor-data notification.
    public struct Record: Equatable {
        public let sensor: Sensor
        /// Device clock, milliseconds, wraps at 65536.
        public let timestamp: Int
        /// x, y, z (vectors, Q4.12) or x, y, z, w (quaternions, Q2.14).
        public let values: [Double]
    }

    private static func i16(_ b: [UInt8], _ at: Int) -> Int { Int(Int16(bitPattern: UInt16(b[at]) << 8 | UInt16(b[at + 1]))) }
    private static func u16(_ b: [UInt8], _ at: Int) -> Int { Int(b[at]) << 8 | Int(b[at + 1]) }

    /// Split a notification into records; several can be packed in one.
    /// Stops at the first record that is unknown or cut short.
    public static func parse(_ b: [UInt8]) -> [Record] {
        var out: [Record] = []
        var at = 0
        while at + 3 <= b.count, let s = Sensor(rawValue: Int(b[at])), at + 3 + s.payloadLength <= b.count {
            let p = at + 3
            let quat = s == .rotation || s == .gameRotation
            let n = quat ? 4 : 3
            let scale = quat ? 16384.0 : 4096.0
            out.append(Record(sensor: s, timestamp: u16(b, at + 1), values: (0..<n).map { Double(i16(b, p + $0 * 2)) / scale }))
            at = p + s.payloadLength
        }
        return out
    }

    /// The configuration table: (sensor id, sample period in ms; 0 = off).
    public static func configEntries(_ b: [UInt8]) -> [(id: Int, periodMs: Int)] {
        stride(from: 0, to: b.count - 2, by: 3).map { (Int(b[$0]), u16(b, $0 + 1)) }
    }

    /// The table to write back with `sensor` at `periodMs` and every other
    /// sensor off; nil if the device does not list that sensor.
    public static func config(_ b: [UInt8], enabling sensor: Sensor, periodMs: Int) -> [UInt8]? {
        var out = b
        var found = false
        for at in stride(from: 0, to: b.count - 2, by: 3) {
            let on = Int(b[at]) == sensor.rawValue
            found = found || on
            let p = on ? periodMs : 0
            out[at + 1] = UInt8(p >> 8); out[at + 2] = UInt8(p & 0xff)
        }
        return found ? out : nil
    }

    /// Sample periods (ms) each sensor offers, from the 12-byte information entries.
    public static func samplePeriods(_ b: [UInt8]) -> [Int: [Int]] {
        let all = [320, 160, 80, 40, 20]
        var out: [Int: [Int]] = [:]
        for at in stride(from: 0, to: b.count - 11, by: 12) {
            let mask = u16(b, at + 9)
            out[Int(b[at])] = all.enumerated().filter { mask & (1 << $0.offset) != 0 }.map(\.element)
        }
        return out
    }

    /// Angles of a unit quaternion (x, y, z, w) about its own Z, Y and X axes,
    /// degrees. Which of them is "turn left" on the head is for the device to tell.
    public static func angles(_ q: [Double]) -> (aboutZ: Double, aboutY: Double, aboutX: Double) {
        guard q.count == 4 else { return (0, 0, 0) }
        let (x, y, z, w) = (q[0], q[1], q[2], q[3])
        let d = 180 / Double.pi
        return (atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)) * d,
                asin(max(-1, min(1, 2 * (w * y - z * x)))) * d,
                atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)) * d)
    }
}
