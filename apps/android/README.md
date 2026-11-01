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

1. Install [Shizuku](https://shizuku.rikka.app) and start it (Wireless
   debugging; pair once). Perch needs it to open *other* apps on glasses
   displays — see `docs/ANDROID.md` for why.
2. Open Perch; on the Shizuku card tap to allow. Plug in the glasses, accept
   the USB prompt, **Start**. Status shows `tracking Rokid Max`.
3. **Calibrate** with the glasses on a table (20 s). Saved to app storage.
4. **Add app** → pick an app (portrait or landscape) → it opens on its own
   display in the glasses, straight ahead. Add more; tap a card to focus.
5. **Touchpad** → move the cursor in the focused window; tap, scroll,
   3-finger swipe to switch windows. ⟳ flips portrait/landscape, ✕ closes.
6. No Shizuku? **Mirror phone screen** still gives one floating window.

## Build on this machine without Android Studio

```bash
export JAVA_HOME=~/.local/share/perch-toolchain/jdk21 ANDROID_HOME=~/Android/Sdk
~/.local/share/perch-toolchain/gradle-8.11.1/bin/gradle assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

## Permissions


USB host (runtime prompt), notifications (foreground service), screen capture
(MediaProjection prompt), and — only if you want the laptop to control this
phone — Accessibility + the Perch keyboard (IME). Nothing leaves your LAN.
