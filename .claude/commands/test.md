Run the full local verification exactly as CI does, fix anything red, and summarise.

Steps:
1. `cargo fmt --all --check` (run `cargo fmt --all` if it fails and say so).
2. `cargo clippy --workspace --all-targets -- -D warnings` — fix warnings in files you touched; for pre-existing ones in other files, list them, don't churn.
3. `cargo test` (default members, no features).
4. `cargo check -p perch-daemon --features full-linux` and `cargo check -p perch-ffi --features rokid`.
5. If wire types changed: confirm `wire_format_tests` in perch-proto, `apps/android/.../PostcardTest.kt` and `apps/ipad/PerchTests/PostcardTests.swift` agree byte-for-byte.
Report: what ran, what failed, what you changed. Append a line to `.claude/memory/changelog.md`.
