#!/usr/bin/env bash
# Build an *unsigned* IPA of the iPad app with nothing but SwiftPM and cargo:
#   on macOS with Xcode's iOS SDK (this is what the release workflow runs),
#   on Linux with the Darwin Swift SDK that `xtool setup` installs.
# The result is what `xtool dev build --ipa` produces; install it with
# `xtool install <ipa>`, AltStore or Sideloadly, which sign it for your Apple ID.
#   usage: scripts/ipad-ipa.sh [out.ipa]          (default: dist/perch-ipad-unsigned.ipa)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/dist/perch-ipad-unsigned.ipa}"
case "$OUT" in /*) ;; *) OUT="$PWD/$OUT" ;; esac
APPDIR="$ROOT/apps/ipad"
WORK="$ROOT/target/ipad-ipa"
MIN_IOS="17.0"
rm -rf "$WORK" && mkdir -p "$WORK/wrapper/Sources/Perch-App" "$WORK/Payload/Perch.app" "$(dirname "$OUT")"

# 1. The shared Rust core as a static library for iOS.
(cd "$ROOT" && cargo rustc -p perch-ffi --release --target aarch64-apple-ios --crate-type staticlib)

# 2. An executable around the app library (the library holds `@main`).
cat > "$WORK/wrapper/Package.swift" <<PKG
// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "Perch-Builder",
    platforms: [.iOS("$MIN_IOS")],
    dependencies: [.package(name: "RootPackage", path: "$APPDIR")],
    targets: [
        .executableTarget(
            name: "Perch-App",
            dependencies: [.product(name: "Perch", package: "RootPackage")],
            linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/Frameworks"])]
        ),
    ]
)
PKG
echo "/* the app's entry point is in the Perch library */" > "$WORK/wrapper/Sources/Perch-App/stub.c"

if [ "$(uname -s)" = "Darwin" ]; then
  FLAGS=(--triple "arm64-apple-ios$MIN_IOS" --sdk "$(xcrun --sdk iphoneos --show-sdk-path)")
else
  # As xtool does it: the Darwin SDK bundle's toolset, built with SwiftPM's swiftbuild engine.
  SDK_BUNDLE="${DARWIN_SDK_BUNDLE:-$HOME/.swiftpm/swift-sdks/darwin.artifactbundle}"
  [ -f "$SDK_BUNDLE/toolset-swb.json" ] || { echo "no Darwin Swift SDK at $SDK_BUNDLE: run 'xtool setup' first" >&2; exit 1; }
  FLAGS=(--build-system swiftbuild --triple arm64-apple-ios --toolset "$SDK_BUNDLE/toolset-swb.json")
  export XCODE_EXTRA_PLATFORM_FOLDERS="$SDK_BUNDLE/Developer/Platforms"
  export PATH="$SDK_BUNDLE/toolset/bin:$PATH"
fi
swift build -c release --package-path "$WORK/wrapper" --scratch-path "$WORK/build" "${FLAGS[@]}"
BIN="$(swift build -c release --package-path "$WORK/wrapper" --scratch-path "$WORK/build" "${FLAGS[@]}" --show-bin-path)"

# 3. The bundle: executable, resources, icon, Info.plist.
APP="$WORK/Payload/Perch.app"
cp "$BIN/Perch-App" "$APP/Perch"
if [ "$(uname -s)" = "Darwin" ]; then
  # SwiftPM records the *macOS* SDK's version number in an iOS binary it
  # cross-builds (seen: "sdk 15.5" from Xcode 16.4). UIKit chooses behaviours
  # by that number, so stamp what the Linux build carries and the app was
  # tested with: SDK version = deployment target.
  xcrun vtool -set-build-version ios "$MIN_IOS" "$MIN_IOS" -replace -output "$APP/Perch" "$APP/Perch"
  xcrun vtool -show-build "$APP/Perch" | grep -E 'platform|minos|sdk'
fi
for b in "$BIN"/*.bundle; do [ -d "$b" ] && cp -R "$b" "$APP/"; done
cp "$APPDIR/AppIcon.png" "$APP/AppIcon.png"
BUNDLE_ID="$(sed -n 's/^bundleID: *//p' "$APPDIR/xtool.yml")"
python3 - "$APPDIR/Info.plist" "$APP/Info.plist" "$BUNDLE_ID" "$MIN_IOS" <<'PY'
import plistlib, sys
src, dst, bundle_id, min_ios = sys.argv[1:5]
info = plistlib.load(open(src, 'rb'))
info.update({
    'CFBundleDevelopmentRegion': 'en', 'CFBundleExecutable': 'Perch', 'CFBundleIconFile': 'AppIcon',
    'CFBundleIdentifier': bundle_id, 'CFBundleInfoDictionaryVersion': '6.0', 'CFBundleName': 'Perch',
    'CFBundlePackageType': 'APPL', 'CFBundleSupportedPlatforms': ['iPhoneOS'], 'CFBundleVersion': '1',
    'LSRequiresIPhoneOS': True, 'MinimumOSVersion': min_ios, 'UIDeviceFamily': [1, 2],
    'UIRequiredDeviceCapabilities': ['arm64'],
})
plistlib.dump(info, open(dst, 'wb'))
PY

rm -f "$OUT"
(cd "$WORK" && zip -qry "$OUT" Payload)
echo "Built $OUT (unsigned)"
unzip -l "$OUT" | tail -n +4
