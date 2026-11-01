# Decisions (durable) — append, don't rewrite history

- 2026-10-31 **Rust core + thin platform shells.** All fusion/geometry/routing in
  Rust crates with tests; Android/iOS via `perch-ffi` C ABI. Reason: one
  implementation for 5 platforms; the maths is where bugs hide.
- 2026-10-31 **Mahony AHRS, not Madgwick/EKF.** Integral term doubles as online
  gyro-bias estimation; tunable with two gains; deterministic; trivially
  testable. Magnetometer off by default (Rokid mag is noisy, yaw correction
  can "swim"). Revisit if drift > 1°/min after stationary calibration.
- 2026-10-31 **`ar-drivers` for Rokid, not Rokid's closed SDK.** MIT, pure
  rusb, has Android fd constructor. We vendor libusb (`rusb/vendored`) so
  builds need no system packages. Upstream quirk: enable `bytemuck` feature
  explicitly (their `rokid` feature forgets it).
- 2026-10-31 **Two channels:** JSON/WebSocket (internally-tagged `Message`,
  human-readable, `websocat`-debuggable) and postcard/UDP (externally-tagged
  `StreamPacket`, ≤64-byte poses, loss-tolerant). Postcard cannot encode
  internally-tagged enums, hence `InputEvent` is externally tagged.
- 2026-10-31 **Edge switching is spatial, not screen-based.** Adjacency comes
  from projected quads at the current head pose, so "push the mouse to the
  right" means the window to the right *in the world*.
- 2026-10-31 **Projective quads via homogeneous w.** Vertex = (x·w, y·w, 0, w)
  with w from the homography's third row; GPU perspective-correct
  interpolation then gives exact projective texture mapping in both wgpu and
  GLES2 with the same 6 vertices. Same code path for SBS halves.
- 2026-10-31 **Black clears to transparent.** Optical see-through: background
  is black; windows are the only lit pixels. Never draw a "desktop".
- 2026-10-31 **Headless by default.** `cargo test` with no features must pass
  on CI with no GPU, no USB. `render`, `rokid`, `linux-input`,
  `linux-pipewire` are opt-in; `full-linux` bundles them.
- 2026-10-31 **iPad is controller-only.** iPadOS cannot open the Rokid USB
  IMU interface (no libusb, no MFi). Host mode is Linux/Windows/macOS/Android.
- 2026-10-31 **Pairing token v0.1 = `name#token` suffix in Hello.** Explicitly
  LAN-only; TLS/Noise is roadmap M5. Do not expose 7420/7421 to the internet.
- 2026-10-31 **Calibration reject gate is a config knob, default 5 °/s peak.** A real
  desk gives 2–4 °/s zero-mean vibration peaks at 444 Hz; the bias estimate is a
  mean so vibration doesn't hurt it. A nudge is tens of °/s and still rejects. A
  rejected run must be visible to the user (`Event::Error`), never silent.
- 2026-11-01 **Linux capture = portal + PipeWire, CPU frames first.** Sources from a pick get
  handles `portal:<n>` (index within the backend, stable across a restore because the portal
  returns streams in the same order); the restore token lives in `[capture] restore_token`.
  One PipeWire main loop per stream, latest frame wins. DMA-BUF import is a later optimisation,
  not a prerequisite — 1–3 windows at 1080p copy fine. The engine places picked windows
  (`add_picked_windows`) and is idempotent so restore after restart adds nothing new.
