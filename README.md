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

## Status: v0.1 scaffold — tracking/spatial core real and tested, I/O backends staged

| Layer | State |
|---|---|
| IMU fusion, gyro-bias calibration, re-centre, latency prediction (`perch-tracking`) | **done, unit-tested** |
| Spatial windows, projection, smooth-follow, presets, layouts (`perch-space`) | **done, unit-tested** |
| Wire protocol JSON + postcard (`perch-proto`) | **done, byte-exact tests shared with Kotlin/Swift** |
| mDNS + WebSocket + UDP (`perch-net`) | **done, tested** |
| Rokid Max USB backend via `ar-drivers` (`perch-hal --features rokid`) | **compiles against real driver; needs on-device test** |
| wgpu compositor, mono + SBS stereo (`perch-compositor`) | **done; renders test pattern** |
| Keyboard/mouse routing + edge switching (`perch-input`) | **done, tested; Linux uinput/evdev backend compiles** |
| Daemon `perchd` + CLI `perch` | **done, run with `--mock synthetic` today** |
| Screen capture (PipeWire / WGC / SCK) | scaffold → [M2](docs/ROADMAP.md) |
| Android host app (USB IMU, GL presentation, mirror window) + touchpad | **written, not yet built on a device** |
| iPad touchpad app (SwiftUI) | **written, not yet built** |
| Windows / macOS daemons | shared Rust core; backends → M4 |

See [docs/ROADMAP.md](docs/ROADMAP.md) for milestones and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Quick start (Linux, Ubuntu 24.04+/26.04)

```bash
# 1. toolchain
curl https://sh.rustup.rs -sSf | sh            # Rust ≥ 1.80
sudo apt install build-essential pkg-config     # for the vendored libusb build
cargo install just                              # task runner (optional)

# 2. udev rule so perchd can open the glasses without sudo
sudo cp udev/70-perch-rokid.rules udev/70-perch-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input $USER && newgrp input   # for keyboard/mouse sharing

# 3. build the pure core + daemon (no hardware needed) and run the tests
cargo test

# 4. run without glasses
cargo run -p perch-daemon -- --mock synthetic --config-dir ./dev &
cargo run -p perch-cli -- status
cargo run -p perch-cli -- preset coding
cargo run -p perch-cli -- windows

# 5. real glasses, with rendering on the Rokid display
cargo run -p perch-daemon --features full-linux --release
cargo run -p perch-cli -- calibrate --seconds 30      # glasses flat on a table
cargo run -p perch-cli -- recenter
```

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
