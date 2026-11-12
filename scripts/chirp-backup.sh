#!/usr/bin/env bash
# Download the radio's memory with CHIRP's own driver, in a container, as an independent second backup.
#   scripts/chirp-backup.sh PORT [OUT.img] [CHIRP_MODEL]      e.g. scripts/chirp-backup.sh /dev/cu.usbserial-110
# Read-only: chirpc runs with --download-mmap only. Radio on at its main screen (stock firmware), cable fully seated.
# Needs pyserial on the host (PYTHON=path/to/python to choose one). Worked on a 5RHNRFB-YP07, V0.13 (2026-11-11).
# CHIRP is installed from its GitHub source when the image is first built and is NOT pinned: this is a cross-check
# of radio_read.py, not a reproducible build. The image is byte-identical to radio_read.py's plus CHIRP's metadata trailer.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${1:?serial port}"; OUT="${2:-$ROOT/my-backup/chirp.img}"; MODEL="${3:-Baofeng_5RM}"
PY="${PYTHON:-python3}"; TCP="${TCP_PORT:-7355}"
"$PY" -c 'import serial' 2>/dev/null || { echo "pyserial is missing for $PY (set PYTHON=...)" >&2; exit 2; }
mkdir -p "$(dirname "$OUT")"
docker build -q -t uv5rm-chirp - >/dev/null <<'DOCKER'
FROM python:3.12-slim
RUN apt-get update && apt-get install -y --no-install-recommends git socat && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir "git+https://github.com/kk7ds/chirp"
DOCKER
docker rm -f uv5rm-chirp-run >/dev/null 2>&1 || true
# chirpc hands the driver a plain pyserial port, but this driver calls the .log() method of CHIRP's GUI serial wrapper:
# the shim adds a no-op .log(). CHIRP_TESTENV keeps chirpc's output on the console.
docker run -d --name uv5rm-chirp-run -p "127.0.0.1:$TCP:$TCP" -e CHIRP_TESTENV=1 -e MODEL="$MODEL" -e TCP="$TCP" \
  -v "$(cd "$(dirname "$OUT")" && pwd)":/out uv5rm-chirp sh -c '
    socat TCP-LISTEN:$TCP,reuseaddr PTY,link=/tmp/radio,raw,echo=0 &
    for i in $(seq 100); do [ -e /tmp/radio ] && break; sleep 0.3; done; sleep 1
    rm -f /out/.chirp-tmp.img
    python -c "
import os, sys, serial
serial.Serial.log = lambda self, msg: None
sys.argv = [\"chirpc\", \"-r\", os.environ[\"MODEL\"], \"-s\", \"/tmp/radio\", \"--mmap\", \"/out/.chirp-tmp.img\", \"--download-mmap\"]
from chirp.cli.main import main; main()"' >/dev/null
"$PY" "$ROOT/scripts/serial_bridge.py" "$PORT" "$TCP" & BRIDGE=$!
trap 'kill $BRIDGE 2>/dev/null || true; docker rm -f uv5rm-chirp-run >/dev/null 2>&1 || true' EXIT
for i in $(seq 180); do [ "$(docker inspect -f '{{.State.Running}}' uv5rm-chirp-run 2>/dev/null)" = true ] || break; sleep 1; done
docker logs uv5rm-chirp-run 2>&1 | tr '\r' '\n' | grep -v 'Cloning from radio' | tail -5 || true
wait $BRIDGE 2>/dev/null || true
# chirpc exits 1 after a download even when it worked, so success is: the file exists and is not empty.
TMP="$(dirname "$OUT")/.chirp-tmp.img"
[ -s "$TMP" ] || { echo "CHIRP download FAILED (no image written)" >&2; exit 1; }
mv "$TMP" "$OUT"; echo "wrote $OUT ($(wc -c < "$OUT" | tr -d ' ') bytes) sha256 $(shasum -a 256 "$OUT" | cut -c1-16)"
