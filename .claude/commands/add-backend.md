Add a new platform backend (capture, input, or glasses). Ask which if not stated.

1. Read the trait in the owning crate (`CaptureBackend`/`CaptureSource`, `InputSink`/`InputSource`, or `Glasses`) and `docs/PLATFORMS.md`.
2. Add a feature flag in that crate's Cargo.toml; gate the module; add it to `perch-daemon` features and to `just ci`'s feature matrix.
3. Implement; keep platform-specific types out of the trait signatures.
4. Add a pure-logic unit test if any mapping/parsing is involved (e.g. key tables).
5. Update `docs/PLATFORMS.md` status table and `.claude/memory/decisions.md` if a design choice was made.
