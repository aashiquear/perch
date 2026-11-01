# Hardware log — facts measured on real devices (append with date + firmware)

## Rokid Max (from ar-drivers source, not yet re-measured by us)
- USB VID 0x04d2 PID 0x162f; interrupt-in endpoint 0x82; product string contains "Max".
- Packet type 17 = combined accel/gyro/mag/keys/proximity/vsync/brightness/volume, 64 bytes,
  timestamp in ns (ar-drivers divides by 1000 → µs). Types 2 (misc) and 4 (single sensor) exist on Air.
- Display modes via vendor control transfer: bRequest 0x01, wValue {0 mono, 1 SBS, 3 120Hz mono, 4 120Hz SBS}, wIndex 1.
- Serial: vendor IN 0x81 wValue 0x100. Current mode: vendor IN 0x81 wValue 0 wIndex 1 → byte[1].
- ar-drivers measured: FOV 23° (unit/meaning unclear vs 50° diag spec), display tilt 0.07 rad, display delay 13 ms.
- spidgets-3dof: use 120 Hz mode; 60 Hz adds visible input lag.
- XRLinuxDriver notes: disable the glasses' built-in "stabilizer/anchor" features; be on latest firmware.

## To measure (fill in with `just hw-check`)
- [ ] IMU rate (Hz) on S24 Ultra and on Ubuntu
- [ ] yaw drift °/min, uncalibrated vs after 30 s stationary cal
- [ ] true horizontal FOV at 1920×1080 (procedure in docs/DEVICE-ROKID-MAX.md)
- [ ] whether 1920×1200 mode is exposed by EDID on Linux/Android
- [ ] whether temple buttons arrive as KeyPress on Max (ar-drivers notes "always zero on my Max")
- [ ] S24 Ultra: does DeX auto-start on the glasses display, and can Perch's Presentation take over?

## Dev machine
- Ubuntu (26.04 target), Bosgame M5 (AMD Ryzen AI Max+ 395). Wayland/GNOME assumed → portal/PipeWire capture path.

## 2026-10-31 — Ubuntu hw-check (Rokid Max, serial 1501082348005479, USB bcdDevice 2.00, firmware string not exposed over USB)
- **USB**: 04d2:162f enumerates; `70-perch-rokid.rules` uaccess ACL works (no sudo). Kernel binds
  hid-generic to the sensor interface; perch detaches/re-attaches it cleanly.
- **IMU rate: 444 Hz, not 1 kHz.** dt median 2252 µs, min 2100, max 2406, zero gaps > 5 ms over 45 s.
  (10 s run: 4449 samples; 45 s run: 19986.) `imu_rate_hz` for Max set to 444.
- **Startup burst**: the first ~9 packets after claiming the interface are stale — device
  timestamp ≈ 0.7 s (boot clock), gyro = ±40.04 rad/s on all axes, mag = 0 — then the clock jumps
  to device uptime (15–80 s). `max_dt` skips the jump, but the 9 garbage samples integrate ~0.6 rad
  before it. TODO (M1): drop packets until the timestamp jump / first 50 ms.
- **Gyro, raw, on a desk**: bias x −0.07, **y +15.1**, z −0.95 °/min; 1σ noise ≈ 0.0015–0.0023 rad/s.
  Bias stable to ±1 °/min over 5 s blocks across 45 s. Accel |a| = 9.883 m/s² (scale 0.9925). Mag live
  (≈ −51, −9, −152 after warm-up; zero for the first ~10 ms).
- **Desk vibration**: visibly still, yet raw |gyro| peaks 2–4 °/s (3.85 °/s max) for seconds at a time
  (oscillatory, zero-mean, y+z). The old 2 °/s peak gate rejected calibration 3/3 times, silently.
  Gate is now `tracker.calib_max_rate_deg_s` (default 5); rejections warn + broadcast `Event::Error`.
- **Yaw drift**: uncalibrated **14.4 °/min** (matches y bias). After `perch calibrate --seconds 30`
  (13 319 samples accepted): **0.08 °/min yaw, 0.01 °/min pitch over 60 s** (yaw range 0.15°).
  Offline estimate from the raw recording agreed (≈ 0.2 °/min). M1 target (< 1 °/min) met.
- **Pose stream**: `perch watch` control-channel poses arrive at ~28 Hz (33 ms throttle), as designed.
- **Display**: GNOME sees "LBT Rokid Max" on DP-5. EDID exposes **1920×1200@120/60 and 1920×1080@120/60**
  (61 modes) — answers the 1920×1200 question: yes, on Linux. `perch mode mono120` succeeds and the
  device reads back Mono120, **but the compositor keeps 1920×1080@60**; the vendor mode switch does not
  re-plug the monitor or change the host's selected mode. Set it host-side:
  `gdctl set -L -M eDP-1 -s 1.5 -p -L -M DP-5 -m 1920x1080@120.000 -x 2048 -y 0` (temporary) → 120 Hz confirmed.
- **Render window**: winit on Wayland names monitors by connector ("DP-5"), so the default
  `display.monitor_name = "Rokid"` never matches and perchd draws in a normal window. Set
  `monitor_name = "DP-5"` (perchd now logs the available names). TODO: match on EDID description.
- **uinput**: `/dev/uinput` is `root:input 0660`; user is not in `input` → local injection unavailable.
  Fix: `sudo usermod -aG input $USER` and re-login (or the 70-perch-uinput rule needs `TAG+="uaccess"`).
- **CLI nit**: perch-net client `close()` still ends with "Connection reset without closing handshake" in perchd log.
- Not yet measured: FOV, temple buttons, axis-sign check (needs glasses on a head).

## 2026-11-01 — Linux M2 smoke test
- perchd `--features full-linux` renders fullscreen on DP-5 (with `monitor_name = "DP-5"`), loads the
  saved calibration (Ready at start), 3 test-pattern windows from `perch preset coding`.
- Build deps without sudo: `apt-get download libpipewire-0.3-dev libspa-0.2-dev libclang-common-21-dev`,
  `dpkg -x` into a scratch dir, sed `prefix=` in the .pc files, symlink `libpipewire-0.3.so.0`, then
  `PKG_CONFIG_PATH=<dir>/usr/lib/x86_64-linux-gnu/pkgconfig LIBCLANG_PATH=/usr/lib/llvm-21/lib
  BINDGEN_EXTRA_CLANG_ARGS="-isystem <dir>/usr/lib/llvm-21/lib/clang/21/include"`. Proper fix:
  `sudo apt install libpipewire-0.3-dev libclang-dev`.
- Not yet verified (needs a person at the screen for the portal dialog): `perch pick --multiple`
  → real windows in the glasses; capture latency; 3 windows at 120 Hz.
- 2026-11-01 Portal facts (GNOME 50): window streams negotiate BGRA at the window's buffer size
  (e.g. 4096×2560 on a 1.5× scaled 3072×1920 panel); monitor streams BGRx 1920×1080 for DP-5.
  Portal reports framerate 0/1 (variable). A restore with the saved token blocked >60 s in
  `select_sources`/`start` with nobody at the screen — GNOME likely shows a confirmation dialog
  even for ExplicitlyRevoked tokens; to verify.
- 2026-11-01 Restore with the saved token re-attached 2 window streams in ~150 ms with no dialog
  (the earlier >60 s hang was a stale token from a session the portal no longer had). Real windows
  render; GNOME window streams are 4096×2560 BGRA. Unpaced render loop hit ~4800 fps → paced to 250.
- 2026-11-01 host laptop: Apple T2 internal trackpad is evdev event16, absolute-only (ABS_X -7456..7976 @96/mm, ABS_Y -9283..163 @94/mm, BTN_TOUCH/TOOL_FINGER/DOUBLETAP/TRIPLETAP, BTN_LEFT, no REL axes); internal keyboard has no Scroll Lock. Group changes need a full GNOME re-login (no sg/newgrp on this box).
- 2026-11-01 GNOME 50 portal restore: tokens are stored under app id "" for host apps (perm store table `screencast`); xdg-desktop-portal deletes the entry on lookup and re-saves at session close, so an unclean exit loses it. Window selections are matched by (desktop file, title): Chrome windows never restore because the title follows the active tab. Monitor selections restore reliably.
- 2026-11-01 Mutter (GNOME 50) stream facts: window stream size = logical monitor rect × ceil(scale) (4096×2560 on this 2048×1280@2 panel) with the window drawn at its surface position and transparent elsewhere; RemoteDesktop NotifyPointerMotionAbsolute on a monitor stream takes stream px (Mutter divides by scale), on a window stream takes logical surface px (clamped to buffer bounds, then actor transform).
- 2026-11-01 GNOME 50 Mutter: RecordVirtual + PipeWire fixed-size format → logical monitor `Meta-0` at x=3968 (right of eDP-1 2048 logical + DP-5 1920); negotiates BGRx 1920x1080; RemoteDesktop.Session methods verified via introspection (NotifyPointerMotionAbsolute(s stream_path,d,d), NotifyKeyboardKeycode(u,b), NotifyPointerButton(i,b), NotifyPointerAxisDiscrete(u,i)). eDP-1 is 3072x1920 at scale 1.5.
- 2026-11-01 Rokid Max brightness: not controllable through ar-drivers 0.4.3 (packet reports display_brightness/volume only; no control transfer known). The temple rocker works.
- 2026-11-01 Mutter relative pointer motion (RemoteDesktop virtual pointer): a move whose target lies on no monitor is clamped to the current monitor's edge, not to the nearest monitor; cross-monitor warps must be done in steps that each land on a monitor.
