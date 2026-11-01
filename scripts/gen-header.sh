#!/usr/bin/env bash
# Regenerate crates/perch-ffi/include/perch_ffi.h with cbindgen and show the diff.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cbindgen >/dev/null || cargo install cbindgen
cbindgen --crate perch-ffi --lang c --output /tmp/perch_ffi.generated.h
diff -u crates/perch-ffi/include/perch_ffi.h /tmp/perch_ffi.generated.h || true
echo "generated header at /tmp/perch_ffi.generated.h — merge by hand (the checked-in one has comments)"
