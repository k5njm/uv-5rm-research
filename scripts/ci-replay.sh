#!/usr/bin/env bash
# Replay GitHub Actions jobs locally in a clean, runner-like x86_64 Ubuntu container (Docker/OrbStack).
#   scripts/ci-replay.sh JOB      JOB is one of: gate sim tools hil images web all
#     gate    release gate on the committed .BF/.app files          sim     simulator, unit tests, scenarios (all styles)
#     tools   host tool tests (incl. the release bundle and CI filter tests)    hil   HIL pipeline against a synthetic camera
#     images  overlay builds, core firmware (both variants), size budgets, pinned hashes
#     web     the images job's builds, then the browser test        all     every job above, in that order
# With no argument it prints this and exits (a replay takes minutes and downloads a toolchain; it must be asked for).
# It runs the workflow's own commands (parsed from .github/workflows/ci.yml with ruby, which macOS ships), against a
# copy of the working tree, with Rust `stable` and NO thumbv7em target like a runner. `uses:` steps (checkout, artifact
# upload/download) are emulated or skipped, and the `changes` filter and `if:` gates are not evaluated (the job you name
# runs; scripts/ci_changed.py is tested by firmware/tests/test_ci_filters.py). Not the repro or release workflows: those
# need Docker inside Docker; use scripts/dev.sh repro / bundle. The copy has no version-control history, so a test that
# needs it skips itself. DRY_RUN=1 only writes build-out/ci/job.sh (the steps it would run) and stops.
set -euo pipefail
usage() { sed -n '2,14p' "$0"; }
if [ $# -ne 1 ]; then usage; exit 2; fi
JOB="$1"
case "$JOB" in gate|sim|tools|hil|images|web|all) ;; *) echo "unknown job '$JOB'" >&2; usage; exit 2 ;; esac
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/build-out/ci"; mkdir -p "$OUT"
ruby -ryaml -e '
job = ARGV[0]
y = YAML.load_file(".github/workflows/ci.yml")
out = ["set -euo pipefail", "export GITHUB_STEP_SUMMARY=/dev/null GITHUB_WORKSPACE=/ws"]
add = lambda do |s|
  next unless s["run"]
  out << "echo; echo \"##### STEP: #{s["name"] || s["run"].lines.first.strip}\""
  out << "cd \"$GITHUB_WORKSPACE/#{s["working-directory"] || "."}\""
  out << "(\n#{s["run"]}\n)"
end
all_steps = lambda { |name| y["jobs"][name]["steps"].each { |s| add.call(s) } }
web = lambda do |images_already_built|
  unless images_already_built
    y["jobs"]["images"]["steps"].each { |s| add.call(s) if (s["name"] || "") =~ /Pinned Rust|overlays build|Core firmware builds/ }
  end
  out << "echo; echo \"##### emulating download-artifact: /tmp/out -> /tmp/dl\"; mkdir -p /tmp/dl && cp /tmp/out/* /tmp/dl/"
  all_steps.call("web")
end
case job
when "web" then web.call(false)
when "all" then (%w[gate sim tools hil images].each { |j| all_steps.call(j) }; web.call(true))
else all_steps.call(job)
end
File.write("build-out/ci/job.sh", out.join("\n") + "\n")
' "$JOB"
[ -z "${DRY_RUN:-}" ] || { echo "wrote $OUT/job.sh"; exit 0; }
cat > "$OUT/replay-inner.sh" <<'INNER'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive PIP_BREAK_SYSTEM_PACKAGES=1
apt-get update -qq && apt-get install -y -qq curl git build-essential pkg-config python3 python3-pip ca-certificates nodejs npm llvm >/dev/null
printf '#!/bin/sh\nexec "$@"\n' > /usr/local/bin/sudo && chmod +x /usr/local/bin/sudo
curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null 2>&1
. /root/.cargo/env
mkdir -p /ws && cd /src && tar --exclude=./build-out --exclude=./target --exclude='*/target' --exclude=./.git -cf - . | tar -xf - -C /ws
bash /work-ci/job.sh
INNER
docker run --rm --platform linux/amd64 -v "$ROOT":/src:ro -v "$OUT":/work-ci ubuntu:24.04 bash /work-ci/replay-inner.sh
echo "ci-replay $JOB: all steps passed"
