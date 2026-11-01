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
