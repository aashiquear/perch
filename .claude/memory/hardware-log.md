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

## 2026-11-02 — S24 Ultra (SM-S928B, Android 16 / One UI 8.5) as host, over Wi-Fi adb
- Rokid Max is display 79: modes 1920×1080 @60/120 and 1920×1200 @60/120; Presentation now requests the 120 Hz 1080p mode.
- `cmd display set-brightness V --id 79` sets the glasses' hardware brightness from uid shell (read back confirms). Android reports brightness range 0..1, default 0.5.
- /dev/uhid is group uhid (3011) and shell is a member: the virtual "Perch Gamepad" (vendor 045e / 02fd) gets Vendor_045e_Product_02fd.kl, Sources KEYBOARD|GAMEPAD|JOYSTICK.
- `am stack list` prints RootTask … displayId=N then taskId=N: pkg/cls; `dumpsys window windows` prints mDisplayId= and hex fl= per window (parsers match).
- Injected keys/touches to a trusted display are logged by InputDispatcher as 'Inject key/motion (pid/2000) … d=80' and flip the focused display 0→80; none were dropped.
- GeForce NOW switches to keyboard+mouse mode on any keyboard-sourced key (Back, DPAD); needs controller-sourced d-pad or a real mouse.

## 2026-11-03 — GNOME 50.1 (Ubuntu) monitor hot-plug behaviour
- Whenever the monitor set changes (virtual display added/removed), Mutter moves a fullscreen window that sits on a secondary monitor (Perch's glasses window on DP-5) onto the primary and gives it focus. Seen with and without gdctl rearrangement. Leaving fullscreen ~0.7 s before the change and re-fullscreening ~2.5 s after avoids it (render.rs unpinned_at).
- Rearranging monitors with gdctl (DP-5 below the laptop, newest display first) is no longer done: GNOME's own append-to-the-right layout is stable and the shortcut simply hops N monitors; moving existing virtual displays sent their windows back to the laptop.
- Default `display.monitor_name = "Rokid"` now resolves to the connector through `gdctl show`.

## 2026-11-14 — iPad (M2, iPadOS 27.0.1) with Rokid Max on USB-C, first host-mode check
*Measured by the in-app check (Glasses tab), report sent from the device.*
- iPadOS hands the app an external-display scene for the glasses
  (`windowExternalDisplayNonInteractive`), **1920×1080, scale 1.0, up to 60 Hz**
  (not 120). With the SwiftUI life cycle a scene delegate set in
  `configurationForConnecting` was never called; the app now attaches its own
  `UIWindow` on `UIScene.willConnectNotification` (not yet confirmed on device).
- The glasses are **visible in the IORegistry** to a sandboxed app: one
  `IOUSBHostDevice "Rokid Max"`, five `IOUSBHostInterface`. Vendor/product id
  properties are hidden from the app.
- `IOHIDManagerOpen` → `0xe00002e2` (not permitted, sandbox). Opening the USB
  device itself was not attempted in this round (the probe matched on vendor
  id, which is hidden); second round pending.
- Earphone head tracking (`CMHeadphoneMotionManager`): available on the iPad;
  not exercised yet.

## 2026-11-14 — iPad host-mode check, second round (same iPad, Rokid Max on USB-C)
*Measured on the device; the owner confirmed the card was visible in the glasses.*
- **Display works.** An app-owned `UIWindow` attached on
  `UIScene.willConnectNotification` draws on the glasses at 1920×1080, 60 Hz max.
  Modes iPadOS lists include 1920×1200 and 2560×1440; 120 Hz was not offered.
- **USB sensor is not readable by an app.** `IOServiceOpen` on
  `IOUSBHostDevice "Rokid Max"`, on each of its five interfaces and on the
  legacy `IOUSBDevice` all return `0xe00002e2` (not permitted, sandbox).
  `IOUSBHost.framework` does not exist on iPadOS 27. `IOHIDManagerOpen` is
  refused too. Only the registry names are visible.
- No head-tracking earphones available to the owner, so that path is untested.
- Conclusion: with a sideloaded, free-account app there is no head-motion
  source on the iPad, so no standalone 3DOF host. The remaining Apple-sanctioned
  route is a DriverKit USB driver (paid developer programme, an entitlement
  Apple must grant for the vendor id, built with Xcode on a Mac) — not tried.

## 2026-11-14 — Bose Noise Cancelling Headphones 700 as a head tracker (iPad, Bluetooth LE)
*Measured by the in-app check; raw bytes in the report sent from the device.*
- Advertises as `LE-Bose NC 700 HP` with services FDD2 (Bose AR), FE26, FE03;
  connects without pairing prompts while also being the audio device.
- FDD2 characteristics present: device info 7B61AD83, sensor info 855CB3E7,
  sensor config 5AF38AF6, sensor data 56A72AB8, gesture info/config/data, and
  one more (B30910E9, not in the community description).
- Sensors offered: 0 accelerometer, 1 gyroscope, 2 rotation, 3 game rotation;
  each at 320/160/80/40/20 ms. Writing config `… 03 00 14` (game rotation,
  20 ms, big-endian) is accepted and gives **49 Hz**.
- Data records are `id u8, timestamp u16 BE (ms), x y z w i16 BE / 16384`,
  **packed 3 to 6 per notification** (so packets arrive every 60–120 ms; the
  sensor clock, not arrival time, must time the samples).
- The first few samples after enabling start near identity and then jump to
  the gravity-referenced value; recentre after that.
- Axes, from three marked poses (straight, head left ≈57°, up ≈36°): sensor
  world **Z up**; body **X right, Y backward, Z down** (the headband sat
  ≈15–20° off level, which the recentre removes). Encoded as
  `SensorFrame::bose_ar()` and checked against the recorded marks in tests.

## 2026-11-14 — iPad host, first on-device run (owner's report)
- Web window on the glasses: appears, undistorted, text and video clear;
  YouTube plays. Turning the head moves the window the opposite way: fixed in
  the room. No lag noticed with the Bose tracker at 49 Hz and 20 ms base
  prediction (subjective, seated).
- Scripted click, scroll and typing work; some clicks "not natural" (single
  click works). The page reported fullscreen as unsupported (before the shim).
- Second run, same day (owner's report): address bar, bookmarks and pages
  fine; sign-in through "on iPad" works; a gamepad test page sees both the
  on-screen Perch controller and a Bluetooth controller. xbox.com/play and
  play.geforcenow.com both stop at "add this page to the Home Screen".
- Genuine touch synthesis works on iPadOS 27.0.1 on the external (non-interactive)
  display: `UITouch` private setters + `_touchesEvent` + IOHIDEvent, sent with
  `UIApplication.sendEvent`; hit-testing through the perspective layer transform
  finds the web view; WebKit acts on the taps (device log, 2026-11-14).

