Start a glasses-free dev loop and check the control protocol end to end.

1. `cargo run -p perch-daemon -- --mock synthetic --config-dir ./dev --log debug` in the background.
2. `cargo run -p perch-cli -- status`, then `preset coding`, `windows`, `recenter`, `focus next`, `layout save work`, `layout load work`.
3. `cargo run -p perch-cli -- watch` for ~3 s and confirm `pose` events arrive at ~30 Hz.
4. Stop the daemon. Report anything that errored, and whether `./dev/workspace.json` was written and reloads.
