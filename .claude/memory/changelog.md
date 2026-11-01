# Changelog (Claude Code sessions) — newest first

- 2026-11-01 Flow-row grid (protocol 1.3): window width from source pixels (`px_per_m`), rows re-flow
  and re-centre on every change; `set_window_pixels`; `slot_window` is (row, index). Pointer
  interaction on the glasses: click = focus, drag re-orders with insertion guide + ghost outlines
  (`Workspace::drag_preview`), release outside rows = free placement; `Projector::unproject`.
  New `perch-ui` crate (5×7 bitmap font + control bar) → bottom bar with RECENTER / CENTRE HERE /
  ARRANGE / ROWS / NEARER / FARTHER / PICK / CALIBRATE / CLEAR; UI commands flow through a channel
  into the engine loop.

- 2026-11-01 Slot grid (protocol 1.2): `Grid` in proto + `state.grid`; `perch-space`
  `grid_slot_placement`, `Workspace::{set_grid,set_slot,arrange_grid,next_free_slot}` (persisted);
  commands `set_grid`/`slot_window`/`arrange_grid`; CLI `perch grid|slot|arrange`. Picked windows
  fill free slots first. Pointer overlay drawn on the glasses when the OS pointer wanders onto DP-5
  (Mutter rejects corner-only layouts). Portal picks dedupe by stream id.

- 2026-11-01 Capture bug: each stream dup'd the portal's PipeWire socket and opened its own core →
  render-side streams hung silently (only the probe stream negotiated). Now one `PwSession` thread
  per portal session, streams added via `pw::channel`. Pick no longer stalls rendering (`try_lock`),
  picked windows go to eye level at the yaw where the user looked when issuing `perch pick`, and
  replace `Virtual` preset placeholders. `perch clear`. Daemon default log filter mutes zbus/wgpu noise.

- 2026-11-01 Perceived lag on head turns was the *debug* build generating three 1080p CPU test
  patterns per frame (single-digit fps). TestPattern now rotates a precomputed frame (memcpy);
  render loop logs fps/avg/max every 5 s; release build measures ~198 fps, 5 ms avg. Release
  binaries installed to ~/.local/bin (the old `perch` there predated `pick`).

- 2026-11-01 Linux real pixels (M2 core): `perch-capture::linux` = portal ScreenCast → PipeWire
  (CPU frames), shared `SharedBackend` in perchd, `Command::PickSources`/`perch pick` (protocol 1.1),
  `Engine::add_picked_windows` (gaze-placed, fan right, idempotent), `[capture] restore_token`
  persisted, render loop opens local sources with test-pattern fallback + retry. HAL drops the
  ±40 rad/s startup burst. `perch remove`. CI installs libpipewire-0.3-dev. PipeWire path not yet
  compiled here (headers missing) — see hardware-log.

- 2026-10-31 hw-check on Ubuntu: IMU 444 Hz, calibrated yaw drift 0.08 °/min, 120 Hz via gdctl.
  Calibration reject gate made configurable (`tracker.calib_max_rate_deg_s`, default 5 °/s) because
  desk vibration (2–4 °/s peaks) rejected every run silently; rejections now warn + `Event::Error`.
  Render warn lists available monitor names; `imu_rate_hz` for Max = 444. New tracker test.

- 2026-10-31 v0.1.0 scaffold: 11 crates, Android + iPad apps, docs, CI. 51 unit tests green.
  Real Rokid backend compiles (vendored libusb). Capture backends are scaffolds (M2).
