#!/usr/bin/env bash
# Containerised dev loop (OrbStack/Docker). Usage: scripts/dev.sh <command>
#   image        build the dev image            shell      interactive shell in the container
#   check        everything required before a PR (release gate, unit tests, scenarios + styles, apps, firmware, HIL tests)
#   sim          original simulator suite        unit       cargo test (app logic.rs tests)
#   scenarios    [--update] classic + all styles apps       build every HAM TOOLS style with sizes
#   firmware     build core firmware variants    hil-test   offline HIL pipeline tests
#   release      everything one radio needs for the CURRENT core: build-out/release/ (core .BF, the nine launcher apps and
#                extras built for the current ABI, layout.json, SHA256SUMS), checked by the release gate
#   bf           build the core variants and wrap them as flashable build-out/fw-*.BF (prints the sha256 flash_bf.py needs)
#   hashes       [--update] compare/record sha256 of built images with firmware/expected-hashes.json
#   manifest     write build-out/manifest.json describing the built images (web builder input)
#   provenance   rebuild every shipped .app from source and compare bytes
#   web-test     headless-Chromium test of web/ against the real built images and manifest
#   size-report  where the core firmware's bytes go: by module, by feature, ABI wrappers and who uses them
#   exec "CMD"   run any command in the container (cwd /work)
#   gate         offline release gate (verify_release.py)
#   tools-test   host tool tests against the fake radio / bootloader (firmware/tests)
#   personalize  boot screen from .env (callsign, name, radio, this build): build-out/personal/boot-logo.rgb565 and .png
#   channels     build channels/*.csv into build-out/channels.img (channel_plan.py; then restore_backup.py, see channels/README.md)
#   repro        build for arm64 AND amd64; everything must match except the known arch-sensitive images (fw-bk4819.bin, fw-default.bin)
# Env: PLATFORM=linux/amd64 to run any command under x86 emulation (Rosetta on OrbStack).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${IMAGE:-uv5rm-dev}"
PLATFORM="${PLATFORM:-}"
ARCH_TAG="$( [ -n "$PLATFORM" ] && echo "${PLATFORM#linux/}" || uname -m | sed 's/aarch64/arm64/;s/x86_64/amd64/' )"
STYLES="d74 proxx ft5d"

plat() { [ -n "$PLATFORM" ] && echo "--platform $PLATFORM" || true; }

# An image is stamped with a hash of what it was built from (Dockerfile + pinned toolchains + browser flag), and is rebuilt
# automatically when that changes, so a stale image can never silently build with the wrong toolchain.
stamp() { cat "$ROOT/dev/Dockerfile" "$ROOT/firmware/toolchain.env" | shasum -a 256 | cut -c1-12; echo "-b${WITH_BROWSER:-1}"; }
image() {
  . "$ROOT/firmware/toolchain.env"
  docker build $(plat) --build-arg RUST_TOOLCHAIN="$LEGACY_TOOLCHAIN" --build-arg SMALL_TOOLCHAIN="$SMALL_TOOLCHAIN" \
    --build-arg WITH_BROWSER="${WITH_BROWSER:-1}" --label "stamp=$(stamp | tr -d '\n')" -t "$IMAGE:$ARCH_TAG" -f "$ROOT/dev/Dockerfile" "$ROOT/dev"
}
ensure_image() {
  have="$(docker image inspect --format '{{ index .Config.Labels "stamp" }}' "$IMAGE:$ARCH_TAG" 2>/dev/null || true)"
  [ "$have" = "$(stamp | tr -d '\n')" ] || { echo "dev image $IMAGE:$ARCH_TAG is missing or out of date; rebuilding" >&2; image; }
}

# A git worktree gets its own build cache: two checkouts mounted at /work share paths, so a shared cargo target
# directory goes stale (and two builds at once corrupt it). CACHE_TAG overrides the name.
if [ -z "${CACHE_TAG:-}" ] && [ -f "$ROOT/.git" ]; then CACHE_TAG="wt$(printf %s "$ROOT" | shasum | cut -c1-8)"; fi
CACHE_VOL="uv5rm-target-$ARCH_TAG${CACHE_TAG:+-$CACHE_TAG}"

# run CMD... inside the container; caches in named volumes, per architecture (and per worktree)
run() {
  ensure_image
  docker run --rm $(plat) $([ -t 1 ] && echo -t) \
    -v "$ROOT":/work \
    -v "uv5rm-cargo-$ARCH_TAG":/opt/cargo/registry \
    -v "$CACHE_VOL":/cache \
    -e OUT=/work/build-out/frames \
    "$IMAGE:$ARCH_TAG" bash -c "set -euo pipefail; $*"
}

SIMDIR=firmware/source-changes/simulator
FWDIR=firmware/firmware-source
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

cmd_sim()       { run "mkdir -p /tmp/simout && cd $SIMDIR && CARGO_TARGET_DIR=/cache/sim cargo run --release -- /tmp/simout ../../skull_logo.rgb565 > /tmp/sim.log; tail -2 /tmp/sim.log; grep -qE '^[0-9]+ passed, 0 failed' /tmp/sim.log"; }
cmd_unit()      { run "cd $SIMDIR && CARGO_TARGET_DIR=/cache/sim cargo test --release > /tmp/unit.log 2>&1 || { cat /tmp/unit.log; exit 1; }; grep -E 'test result' /tmp/unit.log"; }
cmd_scenarios() {
  run "cd $SIMDIR && export CARGO_TARGET_DIR=/cache/sim && ./run_scenarios.sh $* && for st in $STYLES; do STYLE=\$st ./run_scenarios.sh $*; done"
}
cmd_apps() {
  run "cd $FWDIR && mkdir -p /work/build-out && for st in classic $STYLES; do f=''; [ \$st != classic ] && f=style-\$st; MKAPP_FEATURES=\$f python3 tools/mkapp.py apps/hamtools --name 'HAM TOOLS' -o /work/build-out/hamtools-\$st.app 2>&1 | grep -E '^\[OK  \]|ERR' | sed \"s/^/\$st: /\"; done; python3 tools/mkapp.py apps/fskprobe --name 'FSK PROBE' -o /work/build-out/fskprobe.app 2>&1 | grep -E '^\[OK  \]|ERR' | sed 's/^/fskprobe: /'; sha256sum /work/build-out/hamtools-*.app /work/build-out/fskprobe.app"
}
# Core firmware is built under linux/amd64 (the CI and upstream-author environment): the BK4819 image built on
# native arm64 differs by 11,163 bytes (AUDIT.md 18), so Apple Silicon must use emulation for images that are pinned.
cmd_firmware() {
  if [ -z "${PLATFORM:-}" ]; then PLATFORM=linux/amd64 "$0" firmware; else _firmware_impl; fi
}
_firmware_impl() {
  # MODE=small (default, nightly, smaller image) or MODE=legacy (stable, as the shipped binaries were built)
  m="${MODE:-small}"
  run "cd firmware && mkdir -p /work/build-out && \
    CARGO_TARGET_DIR=/cache/fw-$m bash tools/build_core.sh $m /work/build-out/fw-default.bin && \
    if grep -q 'bk4819' firmware-source/Cargo.toml; then \
      CARGO_TARGET_DIR=/cache/fwbk-$m bash tools/build_core.sh $m /work/build-out/fw-bk4819.bin bk4819; fi; \
    sha256sum /work/build-out/fw-*.bin"
}
cmd_bf() {
  cmd_firmware >/dev/null
  run "for v in default bk4819; do [ -f build-out/fw-\$v.bin ] && python3 -I firmware/source-changes/wrap_bf.py build-out/fw-\$v.bin build-out/fw-\$v.BF | sed \"s/^/\$v: /\"; done; sha256sum build-out/fw-*.bin build-out/fw-*.BF"
}
cmd_release() {
  cmd_bf >/dev/null
  run "rm -rf build-out/release && mkdir -p build-out/release && cd firmware/tools && python3 build_release.py /work/build-out/release && cd /work && \
    cp build-out/fw-default.bin build-out/fw-default.BF build-out/release/ && \
    python3 -I firmware/tools/verify_release.py --no-committed --apps build-out/release --image build-out/release/fw-default.bin > /tmp/rel.log || { cat /tmp/rel.log; exit 1; }; tail -1 /tmp/rel.log; \
    cd build-out/release && sha256sum fw-default.BF fw-default.bin *.app > SHA256SUMS && cat SHA256SUMS"
}
cmd_hil_test()  { run "python3 hil/test_hil.py > /tmp/hil.log 2>&1 || { tail -30 /tmp/hil.log; exit 1; }; tail -3 /tmp/hil.log"; }
cmd_tools_test() { run "for t in firmware/tests/test_*.py; do echo \"-- \$t\"; python3 -W error::ResourceWarning \$t > /tmp/t.log 2>&1 || { tail -40 /tmp/t.log; exit 1; }; tail -3 /tmp/t.log | head -2; done"; }
cmd_hashes()    { cmd_apps >/dev/null; cmd_firmware >/dev/null; run "python3 firmware/tools/pin_hashes.py build-out $*"; }
cmd_web_test()  { cmd_apps >/dev/null; cmd_firmware >/dev/null; run "python3 firmware/tools/make_manifest.py build-out build-out/manifest.json >/dev/null && rm -rf /tmp/served && mkdir /tmp/served && cp web/index.html web/builder.js build-out/manifest.json build-out/*.app build-out/*.bin /tmp/served/ && node web/test/run.cjs /tmp/served"; }
cmd_size_report() { cmd_firmware >/dev/null; for t in by_module by_feature api_usage; do echo "=== $t"; run "python3 firmware/tools/size/$t.py"; done; }
cmd_gate()      { run "python3 -I firmware/tools/verify_release.py > /tmp/gate.log || { cat /tmp/gate.log; exit 1; }; tail -1 /tmp/gate.log"; }

cmd_check() {
  step "release gate";           cmd_gate
  step "app unit tests";         cmd_unit
  step "scenarios + styles";     cmd_scenarios
  step "simulator suite";        cmd_sim
  step "HAM TOOLS overlays";     cmd_apps
  step "core firmware";          cmd_firmware
  step "size budgets on built images"; run "python3 -I firmware/tools/verify_release.py --no-committed --apps build-out --image build-out/fw-default.bin --image build-out/fw-bk4819.bin > /tmp/g.log || { cat /tmp/g.log; exit 1; }; tail -1 /tmp/g.log"
  step "shipped apps rebuild from source"; run "python3 firmware/tools/provenance.py --strict | tail -3"
  step "release set: every launcher app built for this core, arena floors"; cmd_release | grep -E "PASSED|FAIL|chanmgr: "
  step "pinned image hashes";    cmd_hashes
  step "host tool tests (fake radio / bootloader)"; cmd_tools_test
  step "web builder page (headless Chromium)"; cmd_web_test
  step "HIL pipeline (offline)"; cmd_hil_test
  printf '\n\033[1;32mall checks passed\033[0m\n'
}

cmd_repro() {
  rm -rf "$ROOT"/build-out/repro-*
  for p in linux/arm64 linux/amd64; do
    d="$ROOT/build-out/repro-${p#linux/}"; mkdir -p "$d"
    step "build under $p"
    # re-invoke so the image tag and caches are chosen for that platform
    PLATFORM=$p "$0" firmware | sed 's/^/  /' || { echo "build failed under $p"; exit 1; }
    cp "$ROOT"/build-out/fw-*.bin "$d/"
    PLATFORM=$p "$0" apps >/dev/null || { echo "app build failed under $p"; exit 1; }
    cp "$ROOT"/build-out/hamtools-*.app "$ROOT"/build-out/fskprobe.app "$d/"
  done
  step "compare"
  # Images known to differ between architectures (AUDIT.md 18). They are reported, not failed; if one starts
  # matching, or any OTHER file differs, this command fails so the list cannot go stale silently.
  KNOWN="fw-bk4819.bin fw-default.bin"
  bad=0
  for f in $(cd "$ROOT/build-out/repro-amd64" && ls); do
    a="$ROOT/build-out/repro-arm64/$f"; b="$ROOT/build-out/repro-amd64/$f"
    if cmp -s "$a" "$b"; then
      case " $KNOWN " in *" $f "*) echo "UNEXPECTED: $f now matches across architectures; remove it from KNOWN"; bad=1 ;; *) echo "  identical  $f" ;; esac
    else
      case " $KNOWN " in *" $f "*) echo "  DIVERGES   $f (known, AUDIT 18): arm64 $(shasum -a 256 "$a" | cut -c1-12) vs amd64 $(shasum -a 256 "$b" | cut -c1-12)" ;; *) echo "  DIFFERENT  $f (NEW divergence)"; bad=1 ;; esac
    fi
  done
  [ $bad = 0 ] && echo "repro OK: everything matches except the known architecture-sensitive image(s)" || { echo "repro FAILED"; exit 1; }
}

case "${1:-}" in
  image) image ;; shell) ensure_image
         docker run --rm $(plat) -it -v "$ROOT":/work -v "uv5rm-cargo-$ARCH_TAG":/opt/cargo/registry -v "$CACHE_VOL":/cache "$IMAGE:$ARCH_TAG" bash ;;
  check) cmd_check ;; sim) cmd_sim ;; unit) cmd_unit ;; scenarios) shift; cmd_scenarios "$@" ;;
  manifest) cmd_apps >/dev/null; cmd_firmware >/dev/null; run "python3 firmware/tools/make_manifest.py build-out build-out/manifest.json && head -c 600 build-out/manifest.json" ;; provenance) run "python3 firmware/tools/provenance.py $*" ;; web-test) cmd_web_test ;; size-report) cmd_size_report ;; exec) shift; run "$*" ;; hashes) shift; cmd_hashes "$@" ;; gate) cmd_gate ;; tools-test) cmd_tools_test ;; apps) cmd_apps ;; firmware) cmd_firmware ;; bf) cmd_bf ;; release) cmd_release ;; hil-test) cmd_hil_test ;; repro) cmd_repro ;;
  personalize) [ -f "$ROOT/.env" ] || { echo "no .env: copy .env.sample to .env and fill it in" >&2; exit 1; }
    ver="$(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet HEAD -- firmware && echo || echo +)"
    run "mkdir -p build-out/personal && cd $SIMDIR && CARGO_TARGET_DIR=/cache/sim cargo run --release -q -- bootcard /work/.env /work/build-out/personal/boot-logo $ver $(date +%F)" ;;
  channels) run "python3 firmware/source-changes/channel_plan.py build channels -o build-out/channels.img" ;;
  *) sed -n '2,10p' "$0"; exit 1 ;;
esac
