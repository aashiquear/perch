# Perch

**Pin windows in space with Rokid Max glasses — from Linux, Android (incl. Samsung DeX), Windows, macOS, with an iPad/phone as your touchpad.**

The Rokid Max has an IMU but no on-board 3DOF; only the Rokid Station gives you
world-pinned windows. Perch does the same thing on your own machines:

```
   laptop (perchd) ──USB-C──► Rokid Max          phone/iPad (Perch app)
   • reads the IMU (1 kHz)                         • trackpad + keyboard
   • fuses it into a head pose                     • pin / focus / layouts
   • pins windows at world directions              • or: phone *is* the host
   • draws them on the glasses (120 Hz)              (USB-C to glasses, DeX or
   • routes one keyboard+mouse to whichever          phone screen as a window)
     machine owns the focused window
```

Primary use: regular laptop coding, design work, watching and gaming while
wearing the glasses, with several windows pinned around you instead of one
bobbing mirror. Step-by-step recipes for each: [docs/USE-CASES.md](docs/USE-CASES.md).

## Status: v0.1.0 — Linux works end to end on GNOME; other hosts staged

| Layer | State |
|---|---|
| IMU fusion, gyro-bias calibration, re-centre, latency prediction (`perch-tracking`) | **done, unit-tested; 0.08°/min drift measured on a Rokid Max** |
| Spatial windows, projection, flow-row grid, drag-to-rearrange, spans (`perch-space`) | **done, unit-tested** |
| Wire protocol JSON + postcard (`perch-proto`, 1.5) | **done, byte-exact tests shared with Kotlin/Swift** |
| mDNS + WebSocket + UDP (`perch-net`) | **done, tested** |
| Rokid Max USB backend via `ar-drivers` (`perch-hal --features rokid`) | **done; 444 Hz IMU verified on device** |
| wgpu compositor, mono + SBS stereo, 240 fps paced (`perch-compositor`) | **done, on device** |
| Glasses displays on GNOME: one Mutter virtual monitor per window, RemoteDesktop input (`perch-capture::gnome`) | **done, on device** |
| Portal/PipeWire capture (other Wayland compositors) | **done; input via the RemoteDesktop portal** |
| Keyboard/trackpad grab (evdev, touchpad gestures) + uinput fallback (`perch-input`) | **done, on device** |
| Daemon `perchd` with on-glasses control bar and desktop dashboard; CLI `perch` | **done** |
| Android host app: panel, apps as glasses displays via Shizuku, pop-up touchpad, Bluetooth input ([docs/ANDROID.md](docs/ANDROID.md)) | **working on an S24 Ultra: 3DOF, multi-app, touchpad, keyboard** |
| iPad touchpad app (SwiftUI) | written, not yet built |
| Windows / macOS daemons | shared Rust core; backends → M4 |

See [docs/ROADMAP.md](docs/ROADMAP.md) for milestones and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Install (users)

### Linux — Ubuntu 24.04+/26.04, GNOME on Wayland

1. Download `perch-vX.Y.Z-linux-x86_64.tar.gz` from the latest
   [GitHub release](https://github.com/aashiquear/perch/releases) and unpack it:

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
3. Plug in the Rokid Max; GNOME shows it as a second monitor. Run `perchd`
   (it writes `~/.config/perch/perchd.toml` on first start; set
   `display.monitor_name` to the glasses' connector, e.g. `"DP-5"`, if it
   picks the wrong one). The dashboard window opens on the laptop.
4. First time only: glasses flat on a table, `perch calibrate --seconds 30`.

With the glasses on:

| do | how |
|---|---|
| move the focused app to the glasses | **Ctrl+Alt+G** (full) / **Ctrl+Alt+H** (half width), or "To glasses" on the dashboard |
| work in it: mouse + keyboard go to the glasses | **Ctrl+Alt+P** (again to come back) |
| re-centre the view | temple button, or RECENTER on the bar |
| nearer / further / bigger / smaller / pin | the bar under the focused window, or the dashboard's window card |
| bring an app back / close it / resize its display | bar tiles, dashboard, or `perch remove`, `perch close-app`, `perch resize` |
| sound to the glasses, natural scrolling | dashboard → Sound & input |

Each glasses window is a virtual monitor GNOME creates for Perch; quitting
`perchd` returns every app to the laptop. See [docs/UX.md](docs/UX.md) and
[docs/USE-CASES.md](docs/USE-CASES.md).

### Android — phone as the host (Samsung S24 Ultra tested)

Prerequisites:

- A phone with USB-C DisplayPort output (the Rokid Max shows an image when
  plugged in) running Android 14 or newer.
- [Shizuku](https://shizuku.rikka.app) from the Play Store. Perch uses it to
  open *other* apps on glasses displays, which stock Android forbids a normal
  app; see [docs/ANDROID.md](docs/ANDROID.md). Start Shizuku once per boot:
  Settings → Developer options → Wireless debugging → on, then in Shizuku
  "Start via Wireless debugging" (pair once).
- On Samsung: Settings → Connected devices → Samsung DeX → turn off
  "Auto start when HDMI/DP is connected", or DeX takes the glasses.

Install and first run:

1. Download `perch-vX.Y.Z-android.apk` from the release (or build it, below)
   and install it; allow "install unknown apps" if asked.
2. Open Perch → the Shizuku card → **Allow**. The Shizuku tile shows **on**.
3. Plug in the glasses, accept the USB prompt, tap **Start**. The status line
   reads `tracking Rokid Max`; **Calibrate** once with the glasses on a table.
4. **Add app** → pick an app and portrait/landscape → it opens on the glasses
   straight ahead. Add more; tap a card to focus it.
5. **Touchpad** pops up the trackpad: one finger moves the cursor, tap taps,
   two fingers scroll, three-finger swipe switches windows. Its bar has Back,
   next, pin, rotate, bigger/smaller, nearer/further and re-open.
6. A Bluetooth keyboard, mouse or gamepad paired to the phone goes to the
   focused glasses window while Perch is on screen (switch in the panel).
7. ✕ on a window card closes the app and its display; **All back** clears
   everything. Without Shizuku, **Mirror phone screen** still gives one window.

The **Laptop remote** tab turns the phone into a trackpad + keyboard for a
laptop running `perchd` (found over mDNS on the same Wi-Fi).

## Developer workflow

```bash
# toolchain + libraries (Ubuntu)
curl https://sh.rustup.rs -sSf | sh            # Rust ≥ 1.80
sudo apt install build-essential pkg-config libudev-dev libpipewire-0.3-dev libclang-dev

cargo test                                      # pure core + daemon, no hardware
just ci                                         # exactly what GitHub Actions runs
cargo run -p perch-daemon --features full-linux -- --mock synthetic --config-dir ./dev   # no glasses
cargo run -p perch-daemon --features full-linux -- --config-dir ./dev                    # glasses plugged in
cargo run -p perch-cli -- status
just hw-check                                   # IMU rate, drift, display mode → .claude/memory/hardware-log.md
```

Android (no Android Studio needed; see [apps/android/README.md](apps/android/README.md)):

```bash
rustup target add aarch64-linux-android && cargo install cargo-ndk
# JDK 21, Gradle 8.11, Android SDK with NDK 27 + CMake 3.22, then:
cd apps/android && gradle assembleDebug          # builds perch-ffi via cargo-ndk, then the APK
adb install -r app/build/outputs/apk/debug/app-debug.apk
gradle testDebugUnitTest                         # postcard byte parity with Rust
```

Rules of the road (details in [CLAUDE.md](CLAUDE.md)): logic lives in the Rust
crates with tests, platform code is a thin shell; wire-format changes bump
`PROTOCOL_VERSION` and update `docs/PROTOCOL.md`; `cargo test` stays green
without features or hardware.

Releasing: tag `vX.Y.Z` on `main`; `.github/workflows/release.yml` builds the
Linux tarball and attaches it to the GitHub release (`/release` in Claude Code
walks through it). The Android APK is attached by hand from
`apps/android/app/build/outputs/apk/debug/app-debug.apk` until the Android
job is wired into the workflow.

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
