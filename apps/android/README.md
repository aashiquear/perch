# Perch for Android

Two modes in one app (see `docs/PLATFORMS.md` §Android for the full story):

- **Host** — plug the Rokid Max into the phone (USB-C). The app opens the
  IMU, runs the shared Rust tracker at 1 kHz and presents 3DOF windows on the
  glasses. Add your phone screen as a window with one tap. Temple button =
  recenter.
- **Laptop remote** — controller for a laptop running `perchd`: trackpad,
  keyboard, window chips, presets. Found automatically via mDNS.

## Build

```bash
# Rust for Android (once)
rustup target add aarch64-linux-android
cargo install cargo-ndk
# Android Studio Ladybug+ (or JDK 21 + Gradle 8.11 + SDK cmdline-tools) with NDK 27 and CMake 3.22, then:
cd apps/android
./gradlew assembleDebug        # runs `cargo ndk … -p perch-ffi --features rokid` first
# the JNI wrapper links against libperch_ffi.so by SONAME; perch-ffi/build.rs sets it on Android
```

The Gradle `cargoBuild` task writes `libperch_ffi.so` into
`app/src/main/jniLibs/arm64-v8a/`; `perch_jni.cpp` is compiled by CMake and
links against it. If you only want the touchpad, you can skip Rust by
removing the `preBuild` dependency temporarily.

Unit tests: `./gradlew testDebugUnitTest` (postcard byte parity with Rust).

## First run (host mode)

1. Install [Shizuku](https://shizuku.rikka.app) and start it (Wireless
   debugging; pair once). Perch needs it to open *other* apps on glasses
   displays — see `docs/ANDROID.md` for why. On Samsung turn off DeX
   auto-start for external displays.
2. Open Perch; on the Shizuku card tap to allow. Plug in the glasses, accept
   the USB prompt, **Start**. Status shows `tracking Rokid Max`.
3. **Calibrate** with the glasses on a table (20 s). Saved to app storage;
   the tracker keeps refining the bias whenever the glasses are still.
4. **Add app** → pick an app (portrait or landscape) → it opens on its own
   display in the glasses, straight ahead. Add more; tap a card to focus.
   Apps rotate their window as they would a phone (YouTube fullscreen).
5. **Touchpad** → the pop-up: chips for Touchpad / Xbox / Wheel / Flight.
   Pad: one finger moves the cursor, tap, two-finger scroll, three-finger
   swipe to switch window; corner locks for drag mode and spring-back
   scrollers; Touch or Mouse mode. Below: glasses brightness, the control
   bar (back, next, pin, rotate, bigger/smaller, nearer/further, re-open),
   an arrow pad and a keyboard button that types into the focused window.
6. A Bluetooth keyboard / mouse / gamepad paired to the phone goes to the
   focused glasses window while Perch is in front (switch in the panel).
7. ✕ on a window card ends the app's tasks on that display and releases it;
   **All back** does it for every window.
8. No Shizuku? **Mirror phone screen** still gives one floating window.

Debugging without adb: a crash shows as a red card at the top of the panel on
the next launch (from `files/crash.txt`); helper-side failures show under the
buttons with the real exception text. With adb (USB or Wireless debugging),
`adb logcat -s PerchShell UhidGamepad AppWindows GlassesService InputDispatcher`
shows the helper, the controller and where Android delivered each event.

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
