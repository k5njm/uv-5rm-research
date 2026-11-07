#!/bin/sh
# Runs every scenario in tests/scenarios against tests/golden (pass --update to rewrite goldens).
# Usage: run_scenarios.sh [--update]   (builds the sim first; output frames in $OUT)
set -e
cd "$(dirname "$0")"
OUT="${OUT:-/tmp/sim-scenarios}"
cargo build --release >/dev/null
BIN="${CARGO_TARGET_DIR:-target}/release/sim"
fail=0
for f in tests/scenarios/*.txt; do
  "$BIN" scenario "$f" --golden tests/golden --out "$OUT" "$@" || fail=1
done
exit $fail
