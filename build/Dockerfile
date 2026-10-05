FROM debian:bullseye-slim

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:$PATH

# Bullseye is archived by 2026. Use Debian Archive rather than the moving
# deb.debian.org/security indices. Valid-Until is intentionally disabled for EOL suites.
RUN printf '%s\n' \
      'deb [check-valid-until=no] http://archive.debian.org/debian bullseye main' \
      'deb [check-valid-until=no] http://archive.debian.org/debian-security bullseye-security main' \
      > /etc/apt/sources.list \
    && printf 'Acquire::Check-Valid-Until "false";\n' > /etc/apt/apt.conf.d/99archive \
    && dpkg --add-architecture arm64 \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
      ca-certificates curl git patch xz-utils unzip zip jq file \
      build-essential cmake ninja-build pkg-config python3 \
      clang libclang-dev \
      gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross \
      libpipewire-0.3-dev:arm64 libopus-dev:arm64 libasound2-dev:arm64 libudev-dev:arm64 \
      libx11-dev:arm64 libxext-dev:arm64 libxi-dev:arm64 libxrandr-dev:arm64 \
      libxcursor-dev:arm64 libxfixes-dev:arm64 libxrender-dev:arm64 \
      libxkbcommon-dev:arm64 libwayland-dev:arm64 \
    && rm -rf /var/lib/apt/lists/*

ARG RUST_TOOLCHAIN=1.96.0
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain "$RUST_TOOLCHAIN" \
    && rustup target add aarch64-unknown-linux-gnu --toolchain "$RUST_TOOLCHAIN"

WORKDIR /work
