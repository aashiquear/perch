# Testing

## Levels

| Level | How | Where |
|---|---|---|
| Unit (pure Rust) | `cargo test` | every crate; 51 tests today |
| Wire-format parity | byte-exact tests in Rust, Kotlin, Swift | `perch-proto` `wire_format_tests`, `PostcardTest.kt`, `PostcardTests.swift` |
| Integration, no hardware | `perchd --mock synthetic` + `perch` CLI | `/mock` slash command |
| Replay | `perchd --mock tools/fixtures/<name>.jsonl` | fixtures recorded with `perch record` |
| Hardware | `/hw-check` | results logged in `.claude/memory/hardware-log.md` |
| Feature matrix | `just ci` | mirrors `.github/workflows/ci.yml` |

## Recording fixtures

```bash
cargo run -p perch-cli --features rokid -- record tools/fixtures/max-still-60s.jsonl --seconds 60
cargo run -p perch-cli --features rokid -- record tools/fixtures/max-yaw-sweep.jsonl --seconds 20
```

Commit small ones (<2 MB). A still recording lets CI assert drift after
calibration on *real* noise; a sweep lets it assert that turning left gives
+yaw.

## Latency

Until the VSync-based measurement lands (M5): point a 240 fps phone camera at
the glasses and a marker on the frame, nod, count frames between motion onset
and image motion. Target ≤ 20 ms motion-to-photon in mono120.

## What a PR must include

- Tests for any pure-logic change.
- `cargo fmt`, `clippy -D warnings` clean.
- Docs updated if behaviour/protocol changed.
