import XCTest
@testable import PerchCore

final class WebAddressTests: XCTestCase {
    private func u(_ s: String) -> String? { WebAddress.url(for: s)?.absoluteString }

    func testSitesGetHttps() {
        XCTAssertEqual(u("xbox.com/play"), "https://xbox.com/play")
        XCTAssertEqual(u("  play.geforcenow.com "), "https://play.geforcenow.com")
        XCTAssertEqual(u("mybox.tail1234.ts.net"), "https://mybox.tail1234.ts.net")
    }

    func testLocalAndTailnetHostsGetHttp() {
        XCTAssertEqual(u("192.168.1.20"), "http://192.168.1.20")
        XCTAssertEqual(u("100.83.47.42:8080/app"), "http://100.83.47.42:8080/app")
        XCTAssertEqual(u("nas:5000"), "http://nas:5000")
        XCTAssertEqual(u("localhost"), "http://localhost")
        XCTAssertEqual(u("printer.local/status"), "http://printer.local/status")
        XCTAssertEqual(u("mybox.tail1234.ts.net:3000"), "http://mybox.tail1234.ts.net:3000")
    }

    func testAnExplicitSchemeIsKept() {
        XCTAssertEqual(u("http://example.com"), "http://example.com")
        XCTAssertEqual(u("https://10.0.0.5:8443/x"), "https://10.0.0.5:8443/x")
    }

    func testWordsAreASearch() {
        XCTAssertEqual(u("rust lifetimes"), "https://www.google.com/search?q=rust%20lifetimes")
        XCTAssertEqual(u("weather"), "https://www.google.com/search?q=weather")
        XCTAssertEqual(u("what is 3.5 x 2"), "https://www.google.com/search?q=what%20is%203.5%20x%202")
        XCTAssertNil(u("   "))
    }

    func testNotQuiteAnIPAddressIsAHostName() {
        XCTAssertTrue(WebAddress.isIPv4("10.0.0.1"))
        XCTAssertFalse(WebAddress.isIPv4("10.0.0.256"))
        XCTAssertFalse(WebAddress.isIPv4("1.2.3"))
        XCTAssertEqual(u("1.2.3"), "https://1.2.3")
    }

    func testBookmarks() {
        var b = Bookmarks()
        XCTAssertEqual(b.items.first?.name, "Xbox Cloud")
        let n = b.items.count
        b.add(name: "", url: "http://100.83.47.42:8080")
        XCTAssertEqual(b.items.last, Bookmark(name: "100.83.47.42", url: "http://100.83.47.42:8080"))
        b.add(name: "Home server", url: "http://100.83.47.42:8080")     // same address: renamed, not doubled
        XCTAssertEqual(b.items.count, n + 1)
        XCTAssertEqual(b.items.last?.name, "Home server")
        XCTAssertTrue(b.contains(url: "https://www.youtube.com"))
        b.remove(url: "https://www.youtube.com")
        XCTAssertFalse(b.contains(url: "https://www.youtube.com"))
        XCTAssertEqual(Bookmarks(json: b.json), b)
        XCTAssertNil(Bookmarks(json: "not json"))
    }

    func testStandardGamepadMapping() {
        var axes = [Float](repeating: 0, count: 8)
        axes[0] = 0.5; axes[3] = -1; axes[5] = 0.7; axes[6] = -1; axes[7] = 1
        let bits: UInt16 = (1 << 0) | (1 << 3) | (1 << 7) | (1 << 11) | (1 << 14)   // A, X, R1, start, R3
        let g = StandardGamepad.map(axes: axes, buttons: bits)
        XCTAssertEqual(g.axes, [0.5, 0, 0, -1])
        XCTAssertEqual(g.buttons.count, 17)
        XCTAssertEqual(g.buttons, [1, 0, 1, 0, 0, 1, 0, 0.7, 0, 1, 0, 1, 0, 1, 1, 0, 0])
        // A trigger pressed as a button reads fully pressed.
        XCTAssertEqual(StandardGamepad.map(axes: [], buttons: 1 << 8).buttons[6], 1)
    }
}
