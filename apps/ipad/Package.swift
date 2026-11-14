// swift-tools-version: 6.0
// Perch for iPad / iPhone. Built on Linux with xtool (https://xtool.sh) or on a
// Mac with Xcode (`xtool dev generate-xcode-project`, or XcodeGen + project.yml).
//
// `PerchCore` is plain Foundation (wire format, gestures, state parsing) and is
// tested on the build host: `PERCH_CORE_ONLY=1 swift test`. The variable leaves
// the SwiftUI target out, which cannot be compiled for Linux.

import Foundation
import PackageDescription

// Absolute, because xtool builds from a generated wrapper package elsewhere.
let rustLibDir = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().appendingPathComponent("../../target/aarch64-apple-ios/release").standardized.path

let coreOnly = ProcessInfo.processInfo.environment["PERCH_CORE_ONLY"] != nil

var products: [Product] = []
var targets: [Target] = [
    .target(name: "PerchCore", swiftSettings: [.swiftLanguageMode(.v5)]),
    .testTarget(name: "PerchCoreTests", dependencies: ["PerchCore"], swiftSettings: [.swiftLanguageMode(.v5)]),
]
if !coreOnly {
    // An xtool project has exactly one library product: the app.
    products.append(.library(name: "Perch", targets: ["Perch"]))
    // The shared Rust core (tracking, projection) as a static library:
    //   cargo rustc -p perch-ffi --release --target aarch64-apple-ios --crate-type staticlib
    // `just ipad` runs that first. CPerch only carries the C header.
    targets.append(.target(name: "CPerch"))
    targets.append(.target(
        name: "Perch",
        dependencies: ["PerchCore", "CPerch"],
        resources: [.process("Resources")],
        swiftSettings: [.swiftLanguageMode(.v5)],
        linkerSettings: [
            .unsafeFlags(["-L\(rustLibDir)"]),
            .linkedLibrary("perch_ffi"),
        ]
    ))
}

let package = Package(
    name: "Perch",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: products,
    targets: targets
)
