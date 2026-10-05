FROM debian:bullseye-slim

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:$PATH

RUN dpkg --add-architecture arm64 \
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
