# Perch for Android

Two modes in one app (see `docs/PLATFORMS.md` §Android for the full story):

- **Host** — plug the Rokid Max into the phone (USB-C). The app opens the
  IMU, runs the shared Rust tracker at 1 kHz and presents 3DOF windows on the
  glasses. Add your phone screen as a window with one tap. Temple button =
  recenter.
- **Touchpad** — controller for a laptop running `perchd`: trackpad,
  keyboard, window chips, presets. Found automatically via mDNS.

## Build

```bash
# Rust for Android (once)
rustup target add aarch64-linux-android
cargo install cargo-ndk
# Android Studio Ladybug+ with NDK 27 and CMake 3.22 installed, then:
cd apps/android
./gradlew assembleDebug        # runs `cargo ndk … -p perch-ffi --features rokid` first
```

The Gradle `cargoBuild` task writes `libperch_ffi.so` into
`app/src/main/jniLibs/arm64-v8a/`; `perch_jni.cpp` is compiled by CMake and
links against it. If you only want the touchpad, you can skip Rust by
removing the `preBuild` dependency temporarily.

Unit tests: `./gradlew testDebugUnitTest` (postcard byte parity with Rust).

## First run (host mode)

1. Plug in the glasses, accept the USB permission prompt.
2. Tap **Start**. Status shows `tracking Rokid Max`, IMU ≈ 1000 Hz.
3. **Calibrate** with the glasses on a table (20 s). Saved to app storage.
4. **Add: mirror this phone's screen** → accept the capture prompt → the
   phone screen floats dead ahead. Pocket the phone; use a Bluetooth keyboard.
5. For other apps as windows and DeX integration see `docs/PLATFORMS.md`.

## Permissions

USB host (runtime prompt), notifications (foreground service), screen capture
(MediaProjection prompt), and — only if you want the laptop to control this
phone — Accessibility + the Perch keyboard (IME). Nothing leaves your LAN.
