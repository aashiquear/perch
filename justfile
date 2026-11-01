# Perch task runner — `cargo install just`

default: test

# Headless core + daemon tests (what CI runs first)
test:
    cargo test

# Everything CI runs
ci:
    cargo fmt --all --check
    cargo clippy -- -D warnings
    cargo test
    cargo check -p perch-daemon --features full-linux
    cargo check -p perch-ffi --features rokid
    cargo test -p perch-compositor -p perch-capture

# Full Linux daemon with real glasses + rendering + input sharing
build-full:
    cargo build --release -p perch-daemon -p perch-cli --features perch-daemon/full-linux,perch-cli/rokid

run-full:
    cargo run --release -p perch-daemon --features full-linux

# No glasses: synthetic IMU, state in ./dev
mock:
    cargo run -p perch-daemon -- --mock synthetic --config-dir ./dev --log debug

# Replay a recording
replay file:
    cargo run -p perch-daemon -- --mock {{file}} --config-dir ./dev

# Install udev rules (Linux)
udev:
    sudo cp udev/*.rules /etc/udev/rules.d/
    sudo udevadm control --reload-rules && sudo udevadm trigger
    @echo "add yourself to the input group: sudo usermod -aG input $USER"

# With glasses plugged in: rate, drift, mode
hw-check:
    cargo run -p perch-cli --features rokid -- record /tmp/perch-imu.jsonl --seconds 10
    @echo "now: just run-full, then 'perch calibrate --seconds 30' and 'perch watch'"

# Android: build Rust for arm64 and the APK
android:
    cd apps/android && ./gradlew assembleDebug

# Regenerate perch_ffi.h with cbindgen and diff against the checked-in header
header:
    scripts/gen-header.sh

# Make the distributable zip
zip:
    scripts/make-zip.sh
