# Contributing

1. Read `CLAUDE.md` (it's the project brief for humans too) and `docs/ARCHITECTURE.md`.
2. `just ci` before pushing.
3. Pure logic → Rust with tests. Platform I/O → thin shells.
4. Protocol changes: bump `PROTOCOL_VERSION`, update `docs/PROTOCOL.md` and the three byte-exact tests.
5. Hardware findings go in `.claude/memory/hardware-log.md` with date + firmware.
6. Commit style: `area: what (why)` — e.g. `tracking: lower ki to 0.01 (over-corrected on nod)`.
