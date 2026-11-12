#!/usr/bin/env bash
# Regenerate the documentation screenshots (docs/img/*.png) from the simulator's golden frames.
#
# Each golden frame is a 160x128 PNG (the radio's screen). The documentation shows it enlarged 3x (480x384).
# The enlarged copies are committed, so nobody needs to run this unless a screen changes: then run
# `scripts/dev.sh check` (or the simulator's run_scenarios.sh) to update the golden frames, look at them,
# and run this script again.
#
# macOS only: it uses `sips`, the image tool that ships with macOS. Nothing else is needed.
#
# To add a screenshot: add one "scenario/shot  output-name" line to FRAMES below, then refer to
# `img/<output-name>.png` from docs/USER-GUIDE.md or README.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDEN="$ROOT/firmware/source-changes/simulator/tests/golden"
OUT="$ROOT/docs/img"

# source frame (golden/<scenario>/<shot>.png)   output name (docs/img/<name>.png)
FRAMES=(
  # CHAN MGR
  "chanmgr_groups/repeaters                  chanmgr-group-list"
  "chanmgr_groups/simplex_empty                 chanmgr-group-empty"
  "chanmgr_edit/editor                       chanmgr-channel-editor"
  "chanmgr_name_stock_blank/typed_k5r        chanmgr-naming"
  "chanmgr_save_notice/saved_notice          chanmgr-saved"
  # SETTINGS
  "settings_menu_groups/groups               settings-groups"
  "settings_menu_groups/autolk_editing       settings-editing"
  "settings_menu_keys/vm_long_app4           settings-keys"
  "settings_system_items/system_items        settings-system"
  # Apps
  "spectrum_two_signals/waterfall_filled     spectrum"
  "bandsweep_vhf/after_first_pass            bandsweep"
  "foxhunt_signal/meter_steady               foxhunt"
  "foxhunt_signal/distance_entry             foxhunt-distance"
  "rptfind_repeater/waiting_for_traffic      rptfinder"
  "rptfind_repeater/save_prompt              rptfinder-save"
  "searchqt_tone/tone_found                  qtscan"
  "searchqt_tone/search_uhf                  search"
  "text_ping/typed_hi                        text"
  "fm_tuner_named/kera                       fm"
  "fm_list/playing_selected                  fm-list"
  "fm_save/save_row                          fm-save"
  "fm_type/typing_two                        fm-typing"
  "hamtools_idtimer/armed_10_00              hamtools-idtimer"
  "hamtools_wavelength/typed_446_000         hamtools-wavelength"
  "fskprobe_idle/on_quiet                    fskprobe"
  "fskprobe_options/all_changed              fskprobe-options"
)

command -v sips >/dev/null || { echo "error: sips not found (this script only runs on macOS)" >&2; exit 1; }
mkdir -p "$OUT"

for pair in "${FRAMES[@]}"; do
  read -r src name <<<"$pair"
  in="$GOLDEN/$src.png"
  out="$OUT/$name.png"
  [[ -f "$in" ]] || { echo "error: missing golden frame $in" >&2; exit 1; }
  sips -z 384 480 "$in" --out "$out" >/dev/null
  echo "wrote docs/img/$name.png  <-  $src.png"
done
echo "${#FRAMES[@]} images written to docs/img/"
