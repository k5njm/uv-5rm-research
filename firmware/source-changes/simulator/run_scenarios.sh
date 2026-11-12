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
# Styles only change the apps that have them (hamtools); other apps are checked once, in the default style.
# The settings app is three crates built with different features, so each of its segments is its own
# simulator build (settings_menu_*, settings_radio_*, settings_system_*); the default build skips them.
if [ -n "$STYLE" ]; then SCEN="tests/scenarios/hamtools_*.txt"; else SCEN=$(ls tests/scenarios/*.txt | grep -v '/settings_'); fi
for f in $SCEN; do
  "$BIN" scenario "$f" --golden "$GOLD" --out "$OUT" "$@" || fail=1
done
if [ -z "$STYLE" ]; then
  for seg in menu radio system; do
    cargo build --release --features "settings-$seg" >/dev/null
    for f in tests/scenarios/settings_${seg}_*.txt; do
      "$BIN" scenario "$f" --golden "$GOLD" --out "$OUT" "$@" || fail=1
    done
  done
fi
exit $fail
