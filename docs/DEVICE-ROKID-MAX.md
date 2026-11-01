# Rokid Max — device notes

Everything here that isn't marked *measured by us* comes from reading
`ar-drivers` 0.4.3 (`src/rokid.rs`), spidgets-3dof's README, and the
XRLinuxDriver wiki. Treat it as "very likely" until `just hw-check` confirms.

## USB

- Two functions over USB-C: DisplayPort alt-mode (the monitor) and a USB 2.0
  vendor device for sensors.
- **VID `0x04d2` PID `0x162f`** (same for Air and Max; the product string
  distinguishes: contains `"Max"`).
- IMU data arrives on **interrupt IN endpoint `0x82`**, 64-byte reports.
  `ar-drivers` claims the interface that owns that endpoint and auto-detaches
  any kernel driver.
- Linux permissions: `udev/70-perch-rokid.rules` (uaccess + plugdev).
- Android: `UsbManager.openDevice()` → `connection.fileDescriptor` →
  `RokidAir::new(fd)` (`ar-drivers` calls `libusb_set_option(NO_DEVICE_DISCOVERY)`
  then `libusb_wrap_sys_device`). Keep the `UsbDeviceConnection` alive.

### Report types

| byte 0 | meaning | notes |
|---|---|---|
| 2 | misc: keys (offset 47), proximity (offset 51) | Air |
| 4 | single sensor: byte1 type (1 acc, 2 gyro, 3 mag), u64 ts @8, f32×3 @20 | Air |
| 17 | **combined** (Max): u64 ts(ns) @1, acc f32×3 @9, gyro @21, mag @33, keys @45, proximity @46, vsync ts @48, brightness @59, volume @60 | ar-drivers: keys "always zero on my Max" |

Units: accel m/s², gyro rad/s right-handed (turning left = +Y), mag µT.
Frame: RUB.

### Control transfers (vendor, device recipient)

| purpose | dir | bRequest | wValue | wIndex | data |
|---|---|---|---|---|---|
| serial | IN | 0x81 | 0x100 | 0 | 64 B, NUL-terminated |
| get display mode | IN | 0x81 | 0 | 1 | byte[1]: 0 mono, 1 SBS, 2 half-SBS, 4 120 Hz SBS, else 120 Hz mono |
| set display mode | OUT | 0x01 | mode | 1 | 1 B zero |

Perch mode names: `mono`=0, `stereo`=1, `mono120`=3, `stereo120`=4.
Changing mode makes the OS see a monitor unplug/replug; the compositor must
survive a resize (it does: `Compositor::resize`).

## Optics

- Panel modes: 1920×1080 @60/120, 1920×1200 @60/120 (if exposed), SBS 3840-wide.
- `ar-drivers`: `display_fov() = 23°` for Max. The spec sheet says 50°
  **diagonal**, which on 16:9 is ≈43.6° horizontal × 24.5° vertical. The 23°
  figure is suspiciously close to the *vertical* FOV; we default
  `display.fov_h_deg = 46` and let you tune.
- Display tilt relative to IMU ≈ 0.07 rad (4°) downward. `display.tilt_deg`.
- Display pipeline delay ≈ 13 ms. Used as `TrackerConfig::base_latency_s`.

### Measuring your FOV (5 minutes)

1. `perch preset single`, `perch place <id> --distance 2 --width 1 --height 0.5625`.
2. Stand 2 m from a wall with a 1 m mark; look at the window edge-on.
3. If the window looks wider than the mark, decrease `fov_h_deg`; narrower,
   increase. Converge until the edges coincide. Write the number in
   `.claude/memory/hardware-log.md`.

## Tracking tips

- **Calibrate once per glasses** with them flat on a table for 30 s
  (`perch calibrate`). Bias is saved per serial.
- 120 Hz mode noticeably reduces perceived lag (spidgets note).
- If the pinned image moves the *wrong* way, the axis convention differs from
  what we expect — XRLinuxDriver exposes `--invert-y --invert-z` for some
  devices; we will add `tracker.invert_axes` if that happens on Max.
- Disable the glasses' own "stabilizer / anchor" features in Rokid's app and
  update firmware (XRLinuxDriver wiki).
- Magnetometer: available in the type-17 packet but noisy; off by default.

## Buttons / sensors

- Temple buttons (volume/brightness rocker) *may* appear as `KeyPress(bit)`.
  Default mapping: button 0 → recenter, button 1 → focus next.
- Proximity → `Worn(bool)`; reserved for auto-dim/pause.
- `VSync` events exist in the combined packet; useful for measuring
  photon-to-pose latency later.
