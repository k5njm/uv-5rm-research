#!/bin/sh
# Runs every scenario in tests/scenarios against the golden frames (pass --update to rewrite them).
#   run_scenarios.sh [--update]            classic style  -> tests/golden
#   STYLE=d74 run_scenarios.sh [--update]  another style  -> tests/golden-d74   (d74, proxx, ft5d)
set -e
cd "$(dirname "$0")"
OUT="${OUT:-/tmp/sim-scenarios}"
FEAT=""; GOLD=tests/golden
if [ -n "$STYLE" ]; then FEAT="--features style-$STYLE"; GOLD="tests/golden-$STYLE"; OUT="$OUT-$STYLE"; fi
cargo build --release $FEAT >/dev/null
BIN="${CARGO_TARGET_DIR:-target}/release/sim"
fail=0
for f in tests/scenarios/*.txt; do
  "$BIN" scenario "$f" --golden "$GOLD" --out "$OUT" "$@" || fail=1
done
exit $fail
