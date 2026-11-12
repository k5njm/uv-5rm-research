#!/usr/bin/env bash
# Replay GitHub Actions jobs locally in a clean, runner-like x86_64 Ubuntu container (Docker/OrbStack).
#   scripts/ci-replay.sh verify     every `run:` step of the verify job
#   scripts/ci-replay.sh web        build the images like CI, then the web job's steps
# It runs the workflow's own commands (parsed from .github/workflows/ci.yml with ruby, which macOS ships), against a
# copy of the working tree, with Rust `stable` and NO thumbv7em target like a runner. `uses:` steps (checkout, artifact
# upload/download) are emulated or skipped. Not the repro job: that needs Docker inside Docker; use scripts/dev.sh repro.
set -euo pipefail
JOB="${1:-verify}"
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
if job == "verify"
  y["jobs"]["verify"]["steps"].each { |s| add.call(s) }
elsif job == "web"
  y["jobs"]["verify"]["steps"].each { |s| add.call(s) if (s["name"] || "") =~ /Pinned Rust|HAM TOOLS overlay|Core firmware builds/ }
  out << "echo; echo \"##### emulating download-artifact: /tmp/out -> /tmp/dl\"; mkdir -p /tmp/dl && cp /tmp/out/* /tmp/dl/"
  y["jobs"]["web"]["steps"].each { |s| add.call(s) }
else
  abort "unknown job #{job}: use verify or web"
end
File.write("build-out/ci/job.sh", out.join("\n") + "\n")
' "$JOB"
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
