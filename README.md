# Perch

**Your Rokid Max becomes a room full of screens.** Perch is a viewing platform
for XR glasses: it turns the glasses from a single bobbing mirror into a set of
windows pinned in space around you, with full 3DOF head tracking, driven by a
Linux laptop or by an Android phone on its own. Open source, no Rokid Station,
no cloud.

Put on the glasses. Your editor floats dead ahead, a terminal to the left, a
browser to the right, a video above. Turn your head and they stay where you
left them; walk away and come back and they are still there. The glasses show
nothing you did not put in them, and everything you did is a real window: the
real app, at native resolution, with your keyboard, mouse, touchpad or
controller going to whichever one you are looking at.

## What it does

- **Multiple windows in 3DOF.** Each window is its own display that an app
  renders into. Perch fuses the glasses' IMU into a head pose and projects
  every window at its world direction, at 120 Hz, with gyro-bias calibration
  and continuous drift correction so windows hold their place.
- **Place them your way.** Pin a window where you are looking, bring it nearer
  or push it further, make it bigger or smaller, flip it portrait or landscape,
  re-centre with the temple button, or pick a layout preset.
- **One set of controls for everything.** On Linux, your keyboard and mouse
  switch between the laptop and the glasses with a chord. On the phone, a
  pop-up touchpad, arrow pad and keyboard drive the focused window, and a
  Bluetooth keyboard, mouse or controller goes straight to it.
- **A controller when you want one.** The phone becomes an Xbox-style pad, a
  racing wheel or a flight stick that games, Chrome and cloud-gaming apps see
  as a real controller.
- **A dashboard on the device you hold.** The same panel on Linux and Android:
  glasses status, a layout map, square one-tap actions, window cards, sound
  output, glasses brightness, light and dark themes.

## Two ways to run it

| | Linux laptop as host | Android phone as host |
|---|---|---|
| Glasses | USB-C to the laptop | USB-C to the phone |
| Windows | any app window, moved to the glasses with **Ctrl+Alt+G**; each is a virtual monitor GNOME creates for Perch | any installed app, picked from a list; each runs on its own display |
| Control | laptop keyboard + trackpad, chord to switch (**Ctrl+Alt+P**), on-glasses bar | pop-up touchpad / mouse mode, d-pad, keyboard bar, Bluetooth devices, virtual gamepad |
| Extras | sound to the glasses, natural scrolling, phone or iPad as a remote trackpad | portrait or landscape per window, apps rotate as on a phone, app close, phone-screen mirror |
| Needs | Ubuntu 24.04+ with GNOME on Wayland | Android 14+, DisplayPort over USB-C, [Shizuku](https://shizuku.rikka.app) |

Both run the same Rust core: tracking, spatial layout, projection and the
wire protocol are shared crates with tests; Kotlin and the daemon are thin
shells around them.

## Status (November 2026)

| | |
|---|---|
| Linux host on GNOME | **v0.1.0 released**: virtual monitors per window, input switching, on-glasses bar, dashboard, audio, presets |
| Android host (Samsung S24 Ultra, Android 16) | **working**: 3DOF, multi-app windows, touchpad, keyboard, mouse, Bluetooth input, virtual gamepad verified in Chrome and GeForce NOW |
| Tracking | 444 Hz IMU, 0.08°/min drift after calibration, online bias learning while still |
| iPad remote | written, not yet built |
| Windows / macOS hosts | shared core ready; backends not started |

Scenarios with step-by-step recipes: [docs/USE-CASES.md](docs/USE-CASES.md).
Design: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/ANDROID.md](docs/ANDROID.md), [docs/ROADMAP.md](docs/ROADMAP.md).

## Install

### Linux — Ubuntu 24.04+/26.04, GNOME on Wayland

1. Download `perch-vX.Y.Z-linux-x86_64.tar.gz` from the latest
   [release](https://github.com/aashiquear/perch/releases) and unpack it:

   ```bash
   tar xzf perch-v*-linux-x86_64.tar.gz && cd perch-v*-linux-x86_64
   sudo install -m755 perchd perch /usr/local/bin/
   ```
2. Let your user reach the glasses and the input devices (once):

   ```bash
   sudo cp udev/70-perch-rokid.rules udev/70-perch-uinput.rules /etc/udev/rules.d/
   sudo udevadm control --reload-rules && sudo udevadm trigger
   sudo usermod -aG input $USER        # log out and back in
   ```
3. Plug in the Rokid Max; GNOME shows it as a second monitor. Run `perchd`.
   It writes `~/.config/perch/perchd.toml` on first start; set
   `display.monitor_name` to the glasses' connector (for example `"DP-5"`) if
   it picks the wrong one. The dashboard opens on the laptop.
4. First time only: glasses flat on a table, `perch calibrate --seconds 30`.

With the glasses on:

| do | how |
|---|---|
| move the focused app to the glasses | **Ctrl+Alt+G** (full) / **Ctrl+Alt+H** (half width), or "To glasses" on the dashboard |
| work in it: mouse + keyboard go to the glasses | **Ctrl+Alt+P** (again to come back) |
| re-centre | temple button, or RECENTER on the bar |
| nearer / further / bigger / smaller / pin | the bar under the focused window, or the window card |
| bring an app back, close it, resize its display | bar tiles, dashboard, or `perch remove`, `perch close-app`, `perch resize` |
| sound to the glasses, natural scrolling | dashboard → Sound & input |

Quitting `perchd` returns every app to the laptop. More in [docs/UX.md](docs/UX.md).

### Android — phone as the host

Prerequisites:

- A phone with DisplayPort output over USB-C (the glasses light up when
  plugged in) on Android 14 or newer. Tested: Samsung S24 Ultra, Android 16.
- [Shizuku](https://shizuku.rikka.app) from the Play Store. Stock Android lets
  an app open only its *own* activities on displays it creates; Shizuku lends
  Perch the shell privilege that lifts that limit, the same way scrcpy works.
  Start it once per boot: Settings → Developer options → Wireless debugging
  → on, then in Shizuku "Start via Wireless debugging" (pair once).
- Samsung: Settings → Connected devices → Samsung DeX → turn off "Auto start
  when HDMI/DP is connected", or DeX takes the glasses.

First run:

1. Install `perch-vX.Y.Z-android.apk` from the release (allow unknown apps).
2. Open Perch → the Shizuku card → **Allow**. The Shizuku tile reads **on**.
3. Plug in the glasses, accept the USB prompt, tap **Start**. The status reads
   `tracking Rokid Max`. **Calibrate** once with the glasses on a table.
4. **Add app** → pick an app and portrait or landscape → it opens on the
   glasses straight ahead. Add more; tap a card to focus it.
5. **Touchpad** opens the control pop-up. Chips switch between Touchpad,
   Xbox, Wheel and Flight. On the pad: one finger moves the cursor, tap taps,
   two fingers scroll, three-finger swipe changes window; the corner locks
   turn every stroke into a drag or show spring-back scrollers; Touch/Mouse
   chooses whether the app sees a finger or a real mouse. Under the pad:
   brightness, the control bar, an arrow pad and a keyboard button that
   types into the focused window.
6. A Bluetooth keyboard, mouse or controller paired with the phone goes to
   the focused window while Perch is on screen (switch in the panel).
7. ✕ on a window card closes the app and its display; **All back** clears
   everything. Without Shizuku, **Mirror** still gives one window of the
   phone screen.

Apps that block screen capture (banking, DRM video) show black; the card
says so. The **Laptop remote** tab turns the phone into a trackpad and
keyboard for `perchd` on a laptop.

## Developer workflow

```bash
curl https://sh.rustup.rs -sSf | sh            # Rust ≥ 1.80
sudo apt install build-essential pkg-config libudev-dev libpipewire-0.3-dev libclang-dev

cargo test                                      # pure core + daemon, no hardware
just ci                                         # exactly what GitHub Actions runs
cargo run -p perch-daemon --features full-linux -- --mock synthetic --config-dir ./dev   # no glasses
cargo run -p perch-daemon --features full-linux -- --config-dir ./dev                    # glasses plugged in
cargo run -p perch-cli -- status
just hw-check                                   # IMU rate, drift, display mode → .claude/memory/hardware-log.md
```

Android, no Android Studio needed ([apps/android/README.md](apps/android/README.md)):

```bash
rustup target add aarch64-linux-android && cargo install cargo-ndk
# JDK 21, Gradle 8.11, Android SDK with NDK 27 + CMake 3.22, then:
cd apps/android && gradle assembleDebug          # builds perch-ffi via cargo-ndk, then the APK
adb install -r app/build/outputs/apk/debug/app-debug.apk
gradle testDebugUnitTest                         # postcard byte parity with Rust
```

Rules of the road ([CLAUDE.md](CLAUDE.md)): logic lives in the Rust crates with
tests and platform code is a thin shell; wire-format changes bump
`PROTOCOL_VERSION` and update `docs/PROTOCOL.md`; `cargo test` stays green
without features or hardware.

Releasing: tag `vX.Y.Z` on `main`; `.github/workflows/release.yml` builds the
Linux tarball and attaches it to the GitHub release. The Android APK is
attached by hand from `apps/android/app/build/outputs/apk/debug/app-debug.apk`
until the Android job is wired into the workflow.

## Repository layout

```
crates/
  perch-proto        wire types (poses, windows, input, commands, events)
  perch-tracking     IMU → pose (Mahony AHRS, bias calibration, prediction)
  perch-space        windows in the world, projection, smooth follow, presets
  perch-hal          glasses backends: rokid (ar-drivers), mock (replay)
  perch-net          mDNS, WebSocket control, UDP stream
  perch-input        input routing/edge switching + Linux uinput/evdev
  perch-capture      screen/window capture sources (+ test pattern)
  perch-compositor   wgpu renderer (mono / SBS) with projective quads
  perch-daemon       perchd
  perch-cli          perch
  perch-ffi          C ABI for Android/iOS
apps/android         Kotlin/Compose host + touchpad app
apps/ipad            SwiftUI touchpad app (XcodeGen spec)
docs/                architecture, protocol, device notes, platforms, roadmap, ADRs
udev/                Linux device permissions
tools/fixtures       IMU recordings for replay tests
.claude/             Claude Code memory + slash commands (see CLAUDE.md)
```

## Working on it with Claude Code

`CLAUDE.md` is the project brief Claude Code reads on every session;
`.claude/memory/` holds durable decisions and device facts;
`.claude/commands/` has `/test`, `/mock`, `/hw-check`, `/release` workflows.

## Credits

- [`ar-drivers`](https://github.com/badicsalex/ar-drivers-rs) (MIT) — Alex Badics'
  open-source Rokid/XREAL driver; Perch's Rokid backend is a thin wrapper.
- [`spidgets-3dof`](https://github.com/3rl-io/spidgets-3dof) /
  [`headset-utils`](https://github.com/3rl-io/headset-utils) (MIT) — proved the
  "euler angles + drift calibration + web widgets" approach; the calibration
  UX here mirrors theirs.
- [XRLinuxDriver / Breezy Desktop](https://github.com/wheaney/XRLinuxDriver) —
  the reference for what a Linux 3DOF desktop should feel like (smooth follow,
  SBS). Rokid support there uses Rokid's closed SDK; Perch stays open.
- [awesome-rokid](https://github.com/Anezium/awesome-rokid) — ecosystem map
  (mostly RV101 AI-glasses projects; useful for the phone-side ideas).

MIT licensed. Not affiliated with Rokid.
