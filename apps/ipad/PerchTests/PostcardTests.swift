import XCTest
@testable import Perch

/// Mirrors `wire_format_tests` in crates/perch-proto.
final class PostcardTests: XCTestCase {
    func testPing() {
        XCTAssertEqual([UInt8](Postcard.ping(1)), [2, 1])
        XCTAssertEqual([UInt8](Postcard.ping(300)), [2, 0xAC, 0x02])
    }

    func testInputBatch() {
        let d = Postcard.input([
            .mouseMove(dx: 1, dy: -2),
            .mouseButton(button: Postcard.btnRight, down: true),
            .scroll(dx: 0, dy: 3),
            .key(usage: 0x04, down: false, mods: 0b0011),
            .text("é"),
        ])
        let expected: [UInt8] = [
            1, 5,
            0, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0xC0,
            2, 1, 1,
            3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x40,
            4, 0x04, 0, 0x03,
            5, 2, 0xC3, 0xA9,
        ]
        XCTAssertEqual([UInt8](d), expected)
    }
}
