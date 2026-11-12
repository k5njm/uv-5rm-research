#!/bin/bash
# EXPERIMENT (nothing in the repo changes): how much smaller does the core get with a nightly toolchain, panic strategy
# immediate-abort and `core` recompiled for size? Run inside the dev container: scripts/dev.sh exec "bash firmware/tools/experiments/nightly_size.sh"
# Result on 2026-11-07 (nightly 1.101.0 of 2026-11-06): core 61,352 -> 57,820 bytes (3,620 free instead of 88).
# NOTE: the flag was renamed this season (panic_immediate_abort feature -> -Cpanic=immediate-abort), so pin an exact nightly date.
set -e
rustup toolchain install nightly --profile minimal -c rust-src -t thumbv7em-none-eabi >/dev/null 2>&1
rm -rf /tmp/fwz && mkdir /tmp/fwz && cp -r /work/firmware/firmware-source /tmp/fwz/ && cd /tmp/fwz/firmware-source && rm -rf target
cargo build --release --target-dir /cache/exp-stable >/dev/null 2>&1
python3 /work/firmware/source-changes/elf2bin.py /cache/exp-stable/thumbv7em-none-eabi/release/bf5rh-fw /tmp/fwz/a.bin | grep ^image | sed 's/^/stable  : /'
sed -i 's/^panic = "abort"$//' Cargo.toml
RUSTFLAGS="-C link-arg=-Tlink.x -Zunstable-options -Cpanic=immediate-abort" cargo +nightly build --release --target-dir /cache/exp-night -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem >/dev/null 2>&1
python3 /work/firmware/source-changes/elf2bin.py /cache/exp-night/thumbv7em-none-eabi/release/bf5rh-fw /tmp/fwz/b.bin | grep ^image | sed 's/^/nightly : /'
