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
- 2026-11-01: protocol 1.5 `send_window{width,height}`; Ctrl+Alt+H half-width displays; grab toggles and sends deferred until Ctrl/Alt are up (stuck modifiers made the keyboard dead on both sides); backend releases held keys on grab release; auto-maximize after send via a gsettings-bound `maximize` chord (Ctrl+Super+Alt+F11).
- 2026-11-01: rename send→move (`move_to_glasses`); `close_app_window`, `resize_display`; bar tiles TO LAPTOP / FULL-HALF / CLOSE APP with hotkeys in labels; panel per-window size list; desktop layout arranged via gdctl (glasses panel below laptop, displays chained right) on every display add/remove so move chords never cross DP-5; focus-click before injected shortcuts; glasses window ignores WM close; pointer re-synced on grab start and parked at laptop centre on release.
- 2026-11-01: GNOME backend keeps a stream-less RemoteDesktop control session (index 0) for pointer parking and keyboard chords, so releasing the grab after the last window went home still brings the OS pointer back; in laptop mode a pointer entering the glasses output is parked back on the laptop (the bar is for glasses mode).
- 2026-11-01: control panel redesigned as a dashboard (glasses picture, mode chip, stat tiles, layout map, big icon buttons, arrange presets); panel 'To glasses' arms a move that fires on the next focus loss so it never moves the panel itself; assets/glasses/rokid-max.png (Rokid product shot, au.rokid.com) embedded via the png crate. Brightness: ar-drivers 0.4.3 has no Rokid set-brightness, only a reported value in CombinedPacket.
- 2026-11-01: pointer parking steps monitor by monitor (Mutter clamps an off-monitor relative move to the current monitor's edge, so one big jump never left the glasses output; coalescing had also summed the two steps). Panel actions are square icon buttons with name + shortcut beside.
- 2026-11-01: new glasses display is placed first in the desktop chain (always one hop; fixes two moves landing on the same display), control-session keyboard warm-up + 350 ms settle (first move used to be lost), presets resize displays (half/full) with picture buttons, dashboard fills its width. release.yml builds Linux x86_64 tarballs on v* tags; README status/quick start rewritten for v0.1.0; repo pushed to github.com/aashiquear/perch.
- 2026-11-01: panel: audio output switch (pw-dump/pw-metadata/wpctl; Rokid Max audio = GPU HDMI/DP sink 'Navi 10 HDMI Audio') and natural/reverse scroll toggle persisted as input.natural_scroll (EvdevSource::set_natural_scroll). CI Android job made non-blocking.
- 2026-11-01: docs/USE-CASES.md (per-scenario recipes), docs/ANDROID.md (phone-as-host design: Shizuku shell service → trusted virtual display per app, am start --display, InputManager injection; cursor drawn by the GL renderer; portrait/landscape via VirtualDisplay.resize). Android app: PerchShellService (AIDL IPerchShell), ShellBridge, AppWindows, PanelScreen (dashboard, app picker, pop-up touchpad), renderer hide/resize/cursor; HAL open_any gated off on Android. Toolchain in ~/.local/share/perch-toolchain (jdk21, gradle 8.11.1); not pushed pending device test.
- 2026-11-01 Android: Start/Start(mock) crashed on the S24 — foreground service declared the mediaProjection type before any capture grant (SecurityException on Android 14+). Now connectedDevice only; mirror grant goes through the service, which promotes itself then calls getMediaProjection. Added a crash card (files/crash.txt) and display list to the panel for adb-less testing.
- 2026-11-01 Android: libperch_jni.so had DT_NEEDED = absolute build path of libperch_ffi.so (no SONAME, CMake IMPORTED target) → UnsatisfiedLinkError on device. perch-ffi build.rs adds -soname on Android; CMake IMPORTED_NO_SONAME.
- 2026-11-01 Android: Shizuku helper createVirtualDisplay failed — system services require the caller's package to belong to uid 2000, and Shizuku's context says "android". Helper now uses a ContextWrapper claiming com.android.shell (scrcpy FakeContext) + hidden DisplayManager(Context) ctor; helper exposes lastError() shown in the panel. FFI mock glasses now paced realtime (was spinning, showed ~1.5 MHz).
- 2026-11-01 Android: pop-up touchpad is a docked overlay, not ModalBottomSheet (sheet drag ate finger motion). Tabs renamed Glasses / Laptop remote.
- 2026-11-01 Android round 2: perch_last_error() → USB open failure text in status; USB device list in status; wm set-ignore-orientation-request per app display (no ROTATES_WITH_CONTENT); nearer/further/bigger/smaller/re-open per window (renderer nudge); touchpad buttons as icons, Home/Recents dropped.
- 2026-11-01 Android round 3: perch_space_placement() so nudges/resizes start from the pinned pose; pin in touchpad bar; hardware keyboard/mouse/gamepad forwarded from the activity to the focused glasses window (injectKeyEvent AIDL id 9), panel switch.
- 2026-11-01 Linux panel restyled to match the Android dashboard: Material-dark theme (apply_theme), rounded full-width cards with section titles, tinted focused window cards, larger text. No behaviour change.
- 2026-11-01 Android round 4: mirror at real screen size/aspect; touchpad bar and window controls wrap (FlowRow, fixed-width Small) and pad height scales with the screen; closeApp (am stack remove on the display, force-stop fallback) so ✕ clears Recents. README: user install (Linux release tarball, Android prerequisites + first run) and developer workflow sections.
- 2026-11-01 Android round 5: start/mirror row wraps; two-finger horizontal scroll (AXIS_HSCROLL); mouse via pointer capture (no phone pointer, no panel clicks); FLAG_SECURE detection (isSecure, AIDL 11) labels blank apps.
- 2026-11-02 Android round 6: drag lock + rim-start drag on the touchpad; d-pad arrow keys; virtual gamepad popup (UHID identity + injected joystick events; Xbox/wheel/flight skins); big coloured Start/Mock/Stop/Mirror; preset pictures; glasses software dimming slider; orientation setting reset on close / phone display.
- 2026-11-02 Android round 7: gamepad merged into the touchpad pop-up (chips Touchpad/Xbox/Wheel/Flight; brightness, controls, d-pad shared); d-pad fixed at the right of the wrapping control row; Xbox skin rebuilt (triggers/bumpers row, two equal sticks + L3/R3, view/menu + hat left, ABXY right, Xbox button); gamepad identity: reuse a physical controller's id when one is paired, re-create the virtual one if it vanished, announce zero state on open, ignore own virtual device in hardware forwarding.
- 2026-11-02 Android: theme action (Device/Light/Dark, persisted in SharedPreferences "theme"); Xbox shoulder buttons 56 dp.
- 2026-11-02 Android: action grid 2×4 without holes; touchpad pop-up scrolls (max 88% height), gamepad skins size themselves; ABXY diamond 54 dp on a 3×3 grid; gamepad diagnostic line (device used, last button inject result).
- 2026-11-02 Android round 8 (first adb session): hardware glasses brightness via shell (fallback software dim); 120 Hz preferred mode; touchpad Touch/Mouse modes (mouse = SOURCE_MOUSE with relative axes + button press/release); two-finger scroll is a finger drag in touch mode; d-pad sends controller DPAD in gamepad modes; drag-lock button moved out of the interop-filtered pad; not-calibrated warning.
- 2026-11-02 Android round 9: gamepad transport = real HID reports through the UHID device (UHID_INPUT2; 13-byte report), injection kept as fallback; Perch window FLAG_NOT_FOCUSABLE while a gamepad skin is up so HID input stays on the game's display; cached physical-gamepad lookup (per-move binder enumeration made the pad sluggish); pointer capture only when a real mouse is paired.
- 2026-11-02 Android round 10: FLAG_NOT_FOCUSABLE removed (caused ANR 'Application does not have a focused window' → kill); Perch forwards its own UHID controller events to the glasses display with the device id intact; touchpad pop-up: pad fixed, only the control section scrolls.
- 2026-11-02 Android round 11: HID gamepad verified in Chrome's gamepad tester (direct kernel path once the pop-up became a non-focusable window). LT/RT drive the analog trigger axes too. Pop-up tightened: single chip row with Touch/Mouse, window chips only when >1, pad 30%, skins wrap content, Xbox L3/R3 beside sticks.
- 2026-11-02 Android round 12: pop-up focusable in Touchpad mode (phone stays IME target → keyboard shows), non-focusable in gamepad modes; pop-up fills the screen below the status bar with imePadding; control section weight(1)+scroll; hardware keys forwarded from the pop-up window via OnUnhandledKeyEventListener; brightness = software dimming again (Rokid ignores Android's stored per-display brightness) plus the stored value.
- 2026-11-02 Tracking: online gyro-bias learning while still (TrackerConfig.auto_bias, 1.5°/s threshold, 2 s window, 15 s tau) — yaw bias is unobservable from gravity and wanders with temperature after a one-off calibration (S24 run still drifted slowly). Test auto_bias_removes_yaw_drift_while_still.
- 2026-11-02 Android round 13: keyboard button under the d-pad opens a typing bar on the phone (text → KeyCharacterMap key events into the focused window, DEL/ENTER), independent of whether the glasses app requested the IME; wheel/flight buttons enlarged.
