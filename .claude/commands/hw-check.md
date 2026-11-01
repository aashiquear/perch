Glasses are plugged in. Validate the hardware path and record findings.

1. `ls /dev/bus/usb` + `lsusb | grep -i 04d2` — confirm the IMU interface is present and the udev rule applies (no sudo needed).
2. `cargo run -p perch-cli --features rokid -- record /tmp/imu.jsonl --seconds 10` → report sample rate.
3. `cargo run -p perch-daemon --features full-linux -- --config-dir ./dev` and in another shell `perch status`, `perch calibrate --seconds 30` (glasses still on a table), then `perch watch` for 60 s: compute yaw drift in °/min from the pose events.
4. `perch mode mono120` — confirm the monitor re-appears at 120 Hz (`wlr-randr`/GNOME display settings).
5. Append measured numbers to `.claude/memory/hardware-log.md` with date and firmware version. If drift > 1°/min after calibration, open an issue note in `docs/ROADMAP.md` under M1.
