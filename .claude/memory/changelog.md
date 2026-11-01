# Changelog (Claude Code sessions) — newest first

- 2026-11-01 M3 core + UI: evdev grab thread (Scroll Lock / GRAB tile / panel toggle), spatial
  pointer forwards clicks/scroll/keys to the app under it (`UiMsg::Input`), uinput absolute
  "Perch Virtual Pointer" device + `AbsTarget` mapping from portal monitor position/size and
  GNOME `DisplayConfig.GetCurrentState`; Alt+drag moves windows. Bar restyled: icon pill with
  hover labels + clock/windows/rows/distance/owner chip (`perch-ui::icons`). Window spans
  (`set_window_span`, `grid.block_w_m`, `perch span`; protocol 1.4). egui control panel window on
  the laptop (pick/arrange/recenter/centre/calibrate/clear/grab/grid sliders/per-window/quit).

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
- 2026-11-01: perch-input touchpad interpreter (abs multitouch pads such as Apple internal trackpad were skipped by the mouse filter so grab captured only the keyboard); Ctrl+Alt+P grab toggle for keyboards without Scroll Lock. Why: user had `input` group but no OS control.
- 2026-11-01: evdev devices set O_NONBLOCK (round-robin poll was parking on the keyboard so touchpad motion only arrived after a key event); capture frames wider than 2560 px are box-downscaled 2x before upload (4096x2560 monitor capture = 42 MB/frame dropped render from 244 to ~80 fps with 90 ms stalls).
- 2026-11-01: while input is grabbed the glasses window ignores compositor pointer events (our own uinput injection echoed back and fought the trackpad); CaptureBackend::close + SIGTERM handling so the portal session closes cleanly and GNOME persists the restore selection; tools/padrec.py touchpad diagnostic.
- 2026-11-01: capture sessions are now xdg RemoteDesktop sessions with screen-cast sources; input to captured apps goes through the portal in stream coordinates (window captures get exact placement). Engine::prune_local_captures drops saved `portal:N`/Display windows at startup (indices restart per run; caused duplicate Chrome windows). Why: window captures had no screen rect so uinput clicks landed elsewhere.
- 2026-11-01: window streams cropped to their opaque bounding box (GNOME window streams are monitor-sized: logical size × ceil(scale)); pointer injection for window streams divides by that integer scale because Mutter maps window-stream positions through the surface in logical px; the grid re-sizes windows from the shown pixels.
- 2026-11-01: GNOME-native backend (perch-capture::gnome): one Mutter virtual monitor per spatial window + RemoteDesktop session for input; Ctrl+Alt+G sends the focused app (injects Super+Shift+Right); removing the window / exiting releases the monitor so apps return to the laptop; pointer moves coalesced and sent no-reply; grab release warps the pointer back left. Why: window captures clamp the pointer to one window, portal round-trips lagged, and the user wants an extended-screen model.
