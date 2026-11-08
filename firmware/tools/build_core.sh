#!/usr/bin/env bash
# Build the core firmware and flatten it to a raw image.
#   build_core.sh small|legacy OUT.bin [cargo-feature]
#     small  : SMALL_TOOLCHAIN (nightly) + panic=immediate-abort + `core` recompiled for size  (the default, smaller image)
#     legacy : LEGACY_TOOLCHAIN (stable), as the shipped binaries were built
# CARGO_TARGET_DIR overrides the build directory. Run from anywhere; prints the image size line from elf2bin.py.
set -euo pipefail
FW="$(cd "$(dirname "$0")/.." && pwd)"
. "$FW/toolchain.env"
mode="${1:?mode: small|legacy}"; out="${2:?output .bin}"; feat="${3:-}"
dir="${CARGO_TARGET_DIR:-/tmp/core-$mode}"
cd "$FW"
args=(--release --manifest-path firmware-source/Cargo.toml --config firmware-source/.cargo/config.toml --target-dir "$dir")
[ -n "$feat" ] && args+=(--features "$feat")
case "$mode" in
  small)  RUSTFLAGS="-C link-arg=-Tlink.x -Zunstable-options -Cpanic=immediate-abort" \
          cargo "+$SMALL_TOOLCHAIN" build "${args[@]}" -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem 2>&1 | tail -1 ;;
  legacy) cargo "+$LEGACY_TOOLCHAIN" build "${args[@]}" 2>&1 | tail -1 ;;
  *) echo "mode must be small or legacy" >&2; exit 2 ;;
esac
python3 -I source-changes/elf2bin.py "$dir/thumbv7em-none-eabi/release/bf5rh-fw" "$out" | tail -2
