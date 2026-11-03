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
- 2026-11-01 **Input grab is explicit, never automatic.** Perch takes the physical devices only
  via the GRAB tile / panel checkbox / Scroll Lock, because once grabbed the laptop is only
  usable through the glasses. Absolute pointer placement uses a separate uinput ABS device
  (0..65535 over the whole desktop, like a VM tablet); relative fallback when the target's
  screen rect is unknown (portal window streams).
- 2026-11-01 Input into captured windows on GNOME Wayland goes through the xdg RemoteDesktop portal (stream-relative NotifyPointerMotionAbsolute), not uinput + window geometry: Mutter exposes no window positions to host apps, and the portal path is the supported one (gnome-remote-desktop uses the same Mutter API). uinput remains for sources without a portal session.
- 2026-11-01 Spatial windows on GNOME are virtual monitors, not window captures. Mutter's session-bus ScreenCast/RemoteDesktop APIs (what gnome-remote-desktop uses) need no dialog for a session app; virtual monitors appear in the layout to the right (connector `Meta-N`) once a PipeWire stream negotiates a fixed size, and vanish when the stream stops, which gives the 'app comes back to the laptop' behaviour for free. Window moves use GNOME's own move-to-monitor shortcut because Mutter exposes no window-move API to apps.
- 2026-11-01 Android multi-app windows use Shizuku (shell uid) trusted virtual displays, the scrcpy --new-display mechanism, because a normal app may only launch its own activities on a virtual display it creates and PUBLIC displays need CAPTURE_VIDEO_OUTPUT. DeX is deliberately not used (Samsung composites DeX itself; no 3DOF).
- 2026-11-02 Decision: gyro bias is refined continuously whenever the glasses are still (auto_bias on by default) instead of relying only on the explicit table calibration; the explicit run is still the fast way to a good starting point and sets Ready state.
