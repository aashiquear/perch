# CLAUDE.md — Perch project brief for Claude Code

You are working in **Perch**, a 3DOF spatial-window system for Rokid Max XR
glasses. Owner: Aashique (Ubuntu + Samsung S24 Ultra + Rokid Max). Read this
file first, then `.claude/memory/*.md` for durable decisions, then the docs a
task touches.

## What Perch is (one paragraph)

The glasses are a dumb 1080p/120 Hz monitor with a 1 kHz IMU on USB. `perchd`
(Rust) reads the IMU, fuses it into a head orientation, keeps a set of
"spatial windows" pinned at world directions, draws them projectively on the
glasses panel, and routes one keyboard+mouse to whichever peer (laptop, phone,
tablet) owns the focused window. Phones/iPads run a touchpad/controller app.
The Android app can also be the host itself (USB-C to glasses, phone screen or
DeX as windows). Same Rust core everywhere via `perch-ffi`.

## Golden rules

1. **Logic lives in Rust, is pure, and has tests.** Platform code (Kotlin,
   Swift, uinput, wgpu) is a thin shell. If you find yourself writing
   geometry/fusion/routing in Kotlin, stop and put it in `perch-space`,
   `perch-tracking`, or `perch-input` and expose it through `perch-ffi`.
2. **Frames & units are fixed:** RUB (X right, Y up, Z back), radians, metres,
   microseconds, quaternion head→world, yaw + = left, pitch + = up. See
   `crates/perch-proto/src/lib.rs` header. Never introduce a second convention.
3. **Wire format is a contract.** JSON control (`Message`, internally tagged)
   and postcard stream (`StreamPacket`, externally tagged). Changing bytes
   means updating `wire_format_tests` in `perch-proto`, `Postcard.kt`,
   `Postcard.swift`, their tests, and `docs/PROTOCOL.md`, and bumping
   `PROTOCOL_VERSION`.
4. **`cargo test` must stay green with no features and no hardware.**
   Hardware/GPU code is feature-gated (`rokid`, `render`, `linux-input`,
   `linux-pipewire`). CI runs `cargo test` and `cargo check` with every
   feature; both must pass.
5. **Don't guess device facts.** Rokid protocol facts come from `ar-drivers`
   (`docs/DEVICE-ROKID-MAX.md`). If something is unknown (e.g. exact FOV),
   say so and make it configurable; don't hard-code a guess as truth.
6. **Latency is the product.** Anything on the IMU → render path must be
   lock-free-ish and allocation-free per sample. Prefer `watch` channels and
   `Arc<Mutex>` held for microseconds over async for that path.
7. **Keep the user's hands free.** UX decisions favour: recenter on the temple
   button, look-to-focus, pin-where-I'm-looking, presets. See `docs/UX.md`.

## Commands you'll use

```bash
cargo test                                  # pure core + daemon (headless)
cargo test --workspace                      # + compositor/capture (needs GPU libs to *link*, not run)
cargo check -p perch-daemon --features full-linux
cargo run -p perch-daemon -- --mock synthetic --config-dir ./dev
cargo run -p perch-cli -- status
just ci                                     # exactly what GitHub Actions runs
just hw-check                               # with glasses plugged in
```

Slash commands in `.claude/commands/`: `/test`, `/mock`, `/hw-check`,
`/add-backend`, `/release`.

## Where things are

| Need to… | Look in |
|---|---|
| change a message / add a command | `crates/perch-proto`, then `engine.rs` in `perch-daemon`, then phone clients |
| tune tracking | `crates/perch-tracking` (`TrackerConfig`), `docs/DEVICE-ROKID-MAX.md` |
| change how windows are placed / projected | `crates/perch-space` |
| add a capture backend | `crates/perch-capture` (trait `CaptureBackend`), `docs/PLATFORMS.md` |
| add an input backend | `crates/perch-input` (traits `InputSink`/`InputSource`) |
| change rendering | `crates/perch-compositor` (`shader.wgsl`, `quad_to_vertices`) |
| daemon wiring | `crates/perch-daemon/src/{device,engine,server,render}.rs` |
| Android | `apps/android/app/src/main/java/io/perch/android/{host,touchpad,input}` |
| iPad | `apps/ipad/Perch/{Net,Touchpad,App}` |
| what's next | `docs/ROADMAP.md` |
| why we did X | `docs/adr/*.md`, `.claude/memory/decisions.md` |

## Testing conventions

- Unit tests next to code. Fixture recordings in `tools/fixtures/*.jsonl`
  (JSON lines: `{"t_us","accel","gyro","mag"?}`), replayed via
  `perch_hal::mock::MockGlasses`.
- A tracking change needs: still-stays-still, pure-yaw-integrates,
  bias-calibration-removes-drift, prediction-moves-ahead (already in
  `perch-tracking`). Extend, don't delete.
- A projection change needs: window-ahead-is-centred, turning-left-moves-right,
  homography-maps-corners (already in `perch-space`).
- Hardware tests are manual: `just hw-check` prints IMU rate, drift over 60 s,
  and display mode. Record results in `.claude/memory/hardware-log.md`.

## Style

Rust 2021, `#![warn(missing_docs)]` on library crates, `thiserror` for error
enums, `tracing` for logs, no `unwrap()` outside tests except on invariants
with a comment. Kotlin: Compose, coroutines/Flow, no RxJava. Swift: SwiftUI +
Network.framework. Keep files under ~500 lines; split by responsibility.

## When you finish a task

1. `cargo test` + the relevant `cargo check --features …`.
2. Update docs that the change invalidates (PROTOCOL/ARCHITECTURE/ROADMAP).
3. Append a line to `.claude/memory/changelog.md` (date, what, why).
4. If you learned a device fact or made a design decision, add it to
   `.claude/memory/hardware-log.md` or `.claude/memory/decisions.md`.
