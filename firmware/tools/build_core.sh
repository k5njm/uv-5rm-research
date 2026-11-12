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
log="$(mktemp)"
case "$mode" in
  small)  RUSTFLAGS="-C link-arg=-Tlink.x -Zunstable-options -Cpanic=immediate-abort" \
          cargo "+$SMALL_TOOLCHAIN" build "${args[@]}" -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem >"$log" 2>&1 || { cat "$log"; exit 1; } ;;
  legacy) cargo "+$LEGACY_TOOLCHAIN" build "${args[@]}" >"$log" 2>&1 || { cat "$log"; exit 1; } ;;
  *) echo "mode must be small or legacy" >&2; exit 2 ;;
esac
tail -1 "$log"
# The app arena must be exactly where the apps are linked to run (abi ARENA_ADDR). A core that puts it anywhere else
# boots and looks fine, then hangs the radio when any app is opened (seen on hardware: AUDIT 31).
elf="$dir/thumbv7em-none-eabi/release/bf5rh-fw"
want="$(sed -n 's/^pub const ARENA_ADDR: usize = 0x\([0-9A-Fa-f_]*\);.*/\1/p' firmware-source/abi/src/lib.rs | tr -d _ | tr 'A-F' 'a-f')"
got="$(rust-nm "$elf" | awk '/overlay5ARENA$/ {print $1}')"
[ -n "$want" ] && [ "$got" = "$want" ] || { echo "ARENA is at 0x$got but apps are linked for 0x$want (abi ARENA_ADDR)" >&2; exit 1; }
echo "arena at 0x$got (matches abi ARENA_ADDR)"
python3 -I source-changes/elf2bin.py "$dir/thumbv7em-none-eabi/release/bf5rh-fw" "$out" | tail -2
