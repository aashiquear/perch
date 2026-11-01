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

Primary use: regular laptop coding, design work, and gaming while wearing the
glasses, with several windows pinned around you instead of one bobbing mirror.

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
| Android host app (USB IMU, GL presentation, mirror window) + touchpad | written, not yet built on a device |
| iPad touchpad app (SwiftUI) | written, not yet built |
| Windows / macOS daemons | shared Rust core; backends → M4 |

See [docs/ROADMAP.md](docs/ROADMAP.md) for milestones and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Quick start (Linux, Ubuntu 24.04+/26.04, GNOME on Wayland)

Prebuilt binaries for x86_64 are attached to each
[GitHub release](https://github.com/aashiquear/perch/releases); or build:

```bash
# 1. toolchain + libraries
curl https://sh.rustup.rs -sSf | sh            # Rust ≥ 1.80
sudo apt install build-essential pkg-config libudev-dev libpipewire-0.3-dev libclang-dev

# 2. udev rules (glasses without sudo) and the input group (trackpad/keyboard grab)
sudo cp udev/70-perch-rokid.rules udev/70-perch-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input $USER                    # then log out and back in

# 3. build + headless tests
cargo test
cargo build --release -p perch-daemon -p perch-cli --features perch-daemon/full-linux

# 4. run with the glasses plugged in (set display.monitor_name in dev/perchd.toml, e.g. "DP-5")
./target/release/perchd --config-dir ./dev
./target/release/perch calibrate --seconds 30   # glasses flat on a table, once
```

Then, with the glasses on:

| do | how |
|---|---|
| move the focused app to the glasses | **Ctrl+Alt+G** (full) / **Ctrl+Alt+H** (half width), or the dashboard |
| work in it: mouse + keyboard go to the glasses | **Ctrl+Alt+P** (again to come back) |
| re-centre the view | temple button, or RECENTER on the bar |
| bring an app back / close it / resize its display | bar tiles, dashboard, or `perch remove`, `perch close-app`, `perch resize` |

Each glasses window is a virtual monitor GNOME creates for Perch; quitting
`perchd` returns every app to the laptop. See [docs/UX.md](docs/UX.md).

Phone/iPad: install the app, it finds `perchd` over mDNS, tap it, you have a
trackpad. See [apps/android/README.md](apps/android/README.md) and
[apps/ipad/README.md](apps/ipad/README.md).

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
