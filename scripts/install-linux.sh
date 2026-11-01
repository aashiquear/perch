#!/usr/bin/env bash
# One-shot setup on Ubuntu: toolchain deps, udev, groups, release build.
set -euo pipefail
cd "$(dirname "$0")/.."
sudo apt-get install -y build-essential pkg-config libudev-dev
sudo cp udev/*.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input,plugdev "$USER" || true
cargo build --release -p perch-daemon -p perch-cli --features perch-daemon/full-linux,perch-cli/rokid
mkdir -p ~/.local/bin
cp target/release/perchd target/release/perch ~/.local/bin/
echo "installed perchd and perch into ~/.local/bin — log out/in for group changes, then run: perchd"
