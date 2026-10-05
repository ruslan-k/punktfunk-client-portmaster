# Source provenance

The build is intentionally source-driven. Version pins live in [`sources.env`](sources.env).

| Component | Source | Pin | Purpose |
| --- | --- | --- | --- |
| Punktfunk | `punktfunk/punktfunk` | `f2596aa2a150f9e83d485c5d5f4a04b53267ba9d` | CLI, protocol, decoder and session renderer |
| SDL3 | `libsdl-org/SDL` | `release-3.4.10` (commit `8e37db5e797b6167f3a00d697d816a684bd259c7`) | embedded aarch64 window/input runtime |
| Rust | rustup | `1.96.0` | exact toolchain used by upstream |
| Build sysroot | Debian | `bullseye-slim` | keeps the generated ELF glibc requirement below the TSPS glibc 2.33 runtime |

## Local patch set

`patches/0001-bullseye-pipewire-compat.patch` drops the upstream `pipewire/v0_3_49` compile-time API gate and uses the existing zero-`requested` fallback in the playback callback.

This patch is deliberately narrow:

- it does not change the Punktfunk wire protocol;
- it does not replace the renderer;
- it does not promise PipeWire audio on SpruceOS;
- when no PipeWire server is available, upstream's playback path fails soft and streaming can continue video-only.

The packaged default decoder is software H.264. That avoids relying on Vulkan Video decode support, but **presentation is still Vulkan**, because that is the current upstream `punktfunk-session` architecture.

## Generated provenance

Every CI package contains `punktfunk/SOURCES.txt`; CI also publishes `build-info.json` and `SHA256SUMS` alongside `punktfunk.zip`.
