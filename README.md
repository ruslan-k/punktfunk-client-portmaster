# Punktfunk client for PortMaster / TrimUI Smart Pro S

Experimental PortMaster packaging for the native [Punktfunk](https://github.com/punktfunk/punktfunk) streaming client, targeting **TrimUI Smart Pro S / aarch64**.

The port intentionally ships the headless `punktfunk` CLI plus the standalone `punktfunk-session` renderer instead of the GTK desktop shell. This keeps the footprint down and avoids GTK/libadwaita on embedded firmware.

## What is included

- reproducible-ish aarch64 cross-build from a pinned Punktfunk commit
- SDL3 built from source and bundled with the port
- compatibility patch for the older PipeWire headers available in the build sysroot
- PortMaster launcher and setup launcher
- persistent Punktfunk trust/settings store on the SD card
- TSPS-oriented Vulkan loader fallback for Mali deployments where `libmali.so` exports Vulkan but `libvulkan.so.1` is absent
- build metadata, source revisions and SHA-256 checksums
- GitHub Actions CI plus automatic `edge` prerelease and stable releases from `v*` tags

## Status

This is an **experimental TSPS port**. Upstream `punktfunk-session` is currently an SDL3 + Vulkan presenter. The build can be validated in CI, but the final Vulkan WSI path has to be validated on the real TrimUI firmware/GPU stack.

The default decoder is software H.264 to avoid assuming Vulkan Video support on the handheld. This does **not** remove the Vulkan requirement for presentation.

## Install

Download `punktfunk.zip` from Releases, then install it as a PortMaster port or extract it into your Ports directory.

Run **Punktfunk Setup** once to pair the handheld with a host, then launch **Punktfunk**.

## Build

```sh
docker build -f build/Dockerfile -t punktfunk-portmaster-builder .
docker run --rm -v "$PWD:/work" punktfunk-portmaster-builder ./scripts/build.sh
```

Artifacts are written to `dist/`.

## Upstream

Punktfunk is dual-licensed MIT OR Apache-2.0. Upstream license and third-party notice files are copied into the generated port package during the build.
