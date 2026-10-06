# Punktfunk client for PortMaster / TrimUI Smart Pro S

Experimental PortMaster packaging for the native [Punktfunk](https://github.com/punktfunk/punktfunk) streaming client, targeting **TrimUI Smart Pro S / aarch64** and the current SpruceOS nightly line.

The port ships the headless `punktfunk` CLI plus the standalone `punktfunk-session` SDL3/Vulkan renderer instead of the GTK desktop shell. This keeps the footprint down and avoids GTK/libadwaita on the handheld.

## Firmware baseline

The current source lock targets SpruceOS nightly **v4.5.1-20261004.2** (development commit `43dcac54c7e31b8b5eda6ce0aa6971a665a2175b`). The exact target is recorded in `sources.env`, `punktfunk/SOURCES.txt`, and every CI `build-info.json`.

## What is included

- pinned Punktfunk and SDL3 sources with reproducible aarch64 cross-build metadata
- SDL3 built from source and bundled with the port
- embedded **ALSA playback backend** replacing upstream desktop PipeWire
- integration with SpruceOS `asound-setup.sh`, including its current speaker/Bluetooth route
- PortMaster launcher plus a pairing/setup launcher
- persistent Punktfunk trust/settings store on the SD card
- software H.264 decode as the conservative default; Vulkan is still used for presentation
- build metadata, source revisions and SHA-256 checksums
- GitHub Actions CI, rolling `edge` prerelease from `main`, and stable releases from `v*` tags

## Embedded differences from upstream

Upstream Linux Punktfunk uses PipeWire for speaker/microphone and DualSense audio. SpruceOS is ALSA-based, so this port replaces only that device-facing layer:

- stream playback: ALSA, S16LE, normally stereo/48 kHz
- microphone uplink: disabled for now
- DualSense speaker / voice-coil audio: disabled for now
- normal SDL gamepad input and rumble: unchanged

The wire protocol, trust store, host discovery/pairing, video decoder, renderer and session logic remain upstream Punktfunk.

## Status

This is still an **experimental TSPS port**. CI verifies the aarch64 build and package structure. The final Vulkan WSI/Mali path and end-to-end latency need validation on real TrimUI hardware after each SpruceOS/Punktfunk bump.

The default decoder is software H.264 so the port does not assume Vulkan Video decode support. That does **not** remove the Vulkan requirement for the current `punktfunk-session` presenter.

## Install

Download `punktfunk.zip` from the **edge** prerelease (or a tagged release) and install/extract it as a PortMaster port.

Run **Punktfunk Setup** once to pair the handheld with a Punktfunk host, then launch **Punktfunk**.

Runtime logs are written under `ports/punktfunk/logs/`.

## Build locally

```sh
docker build -f build/Dockerfile -t punktfunk-portmaster-builder .
docker run --rm -v "$PWD:/work" punktfunk-portmaster-builder bash ./scripts/build.sh
```

Artifacts are written to `dist/`:

- `punktfunk.zip`
- `SHA256SUMS`
- `build-info.json`

## Releases

- every push to `main`: rebuilds and replaces the rolling `edge` prerelease
- every `v*` tag: creates a normal GitHub release
- manual workflow dispatch: can create a named release tag

## Upstream

Punktfunk is dual-licensed MIT OR Apache-2.0. Upstream license and third-party notice files are copied into the generated port package during the build.
