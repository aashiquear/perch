import XCTest
@testable import PerchCore

final class BoseARTests: XCTestCase {
    func testParsesPackedRecords() {
        // game rotation (id 3), t=0x0102, identity quaternion; then gyroscope (id 1), t=0x0103, (1, -1, 0.5), accuracy 3
        let b: [UInt8] = [3, 0x01, 0x02, 0, 0, 0, 0, 0, 0, 0x40, 0x00,
                          1, 0x01, 0x03, 0x10, 0x00, 0xF0, 0x00, 0x08, 0x00, 3]
        let r = BoseAR.parse(b)
        XCTAssertEqual(r, [
            .init(sensor: .gameRotation, timestamp: 258, values: [0, 0, 0, 1]),
            .init(sensor: .gyroscope, timestamp: 259, values: [1, -1, 0.5]),
        ])
    }

    func testStopsAtTruncatedOrUnknownRecord() {
        XCTAssertEqual(BoseAR.parse([3, 0, 0, 0, 0]).count, 0)
        XCTAssertEqual(BoseAR.parse([99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).count, 0)
        XCTAssertEqual(BoseAR.parse([]).count, 0)
    }

    func testConfigEnablesOneSensorOnly() {
        let table: [UInt8] = [0, 0, 20, 1, 0, 0, 2, 0, 0, 3, 0, 0]
        XCTAssertEqual(BoseAR.configEntries(table).map(\.periodMs), [20, 0, 0, 0])
        XCTAssertEqual(BoseAR.config(table, enabling: .gameRotation, periodMs: 20)!, [0, 0, 0, 1, 0, 0, 2, 0, 0, 3, 0, 20])
        XCTAssertEqual(BoseAR.config(table, enabling: .rotation, periodMs: 320)!, [0, 0, 0, 1, 0, 0, 2, 0x01, 0x40, 3, 0, 0])
        XCTAssertNil(BoseAR.config(table, enabling: .magnetometer, periodMs: 20))
    }

    func testSamplePeriodBitmask() {
        var e = [UInt8](repeating: 0, count: 12)
        e[0] = 3; e[10] = 0b11000        // 40 and 20 ms
        XCTAssertEqual(BoseAR.samplePeriods(e), [3: [40, 20]])
    }

    func testAngles() {
        let s = sin(Double.pi / 8), c = cos(Double.pi / 8)   // 45° turn
        XCTAssertEqual(BoseAR.angles([0, 0, s, c]).aboutZ, 45, accuracy: 1e-6)
        XCTAssertEqual(BoseAR.angles([0, s, 0, c]).aboutY, 45, accuracy: 1e-6)
        XCTAssertEqual(BoseAR.angles([s, 0, 0, c]).aboutX, 45, accuracy: 1e-6)
    }
}
