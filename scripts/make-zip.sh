#!/usr/bin/env bash
# Build the distributable archive (source tree, no build artefacts).
set -euo pipefail
cd "$(dirname "$0")/.."
v=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
out="perch-${v}.zip"
rm -f "$out"
git ls-files -z | xargs -0 zip -q "$out"
echo "wrote $out"
