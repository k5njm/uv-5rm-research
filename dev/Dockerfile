# Build/test environment for the UV-5RM firmware, overlay apps, simulator and HIL host tests.
# Nothing here is specific to the host machine. The ARM firmware target cross-compiles from any
# host, so linux/arm64 (native on Apple Silicon) and linux/amd64 (CI) must give identical images;
# `scripts/dev.sh repro` checks that. Hardware-facing steps (flashing, webcam, injector) are NOT
# done here: containers cannot reach the Mac's USB serial ports or camera.
FROM ubuntu:24.04
ARG RUST_TOOLCHAIN=1.99.1
ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/usr/local/bin:/usr/bin:/bin
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git build-essential pkg-config \
        python3 python3-pil python3-numpy python3-serial \
    && rm -rf /var/lib/apt/lists/*
# Rust: pinned toolchain (keep in sync with firmware/firmware-source/rust-toolchain.toml; dev.sh checks)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none \
 && rustup toolchain install ${RUST_TOOLCHAIN} --profile minimal -t thumbv7em-none-eabi -c llvm-tools \
 && rustup default ${RUST_TOOLCHAIN}
# mkapp.py expects rust-objcopy / rust-nm / rust-size on PATH
RUN bin="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin" \
 && for t in objcopy nm size; do ln -sf "$bin/llvm-$t" /usr/local/bin/rust-$t; done \
 && rust-objcopy --version | head -1
# Browser test of web/ (approved: Node + Playwright + headless Chromium). Pinned.
ARG PLAYWRIGHT_VERSION=1.49.1
ENV PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers NODE_PATH=/usr/local/lib/node_modules
RUN apt-get update && apt-get install -y --no-install-recommends nodejs npm \
 && npm install -g playwright@${PLAYWRIGHT_VERSION} \
 && playwright install --with-deps chromium \
 && rm -rf /var/lib/apt/lists/* \
 && node --version && playwright --version
WORKDIR /work
