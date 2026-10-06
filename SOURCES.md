# Source provenance

The build is source-driven. Version pins live in [`sources.env`](sources.env).

| Component | Source | Pin | Purpose |
| --- | --- | --- | --- |
| Punktfunk | `punktfunk/punktfunk` | `f2596aa2a150f9e83d485c5d5f4a04b53267ba9d` | CLI, protocol, decoder and session renderer |
| SDL3 | `libsdl-org/SDL` | `release-3.4.10` | embedded aarch64 window/input runtime |
| Rust | rustup | `1.96.0` | toolchain used for the cross-build |
| Build sysroot | Debian | `bullseye-slim` | keeps generated ELF glibc requirements conservative for TSPS |
| SpruceOS target | `spruceUI/spruceOSNightlies` | `v4.5.1-20261004.2` / `43dcac54c7e31b8b5eda6ce0aa6971a665a2175b` | firmware/API baseline |

## Embedded source substitutions

The PortMaster build copies two maintained source files over the pinned upstream tree before compilation:

- `patches/audio-alsa.rs` replaces the Linux PipeWire client sink with an ALSA playback backend. It uses SpruceOS' generated `default` PCM, keeps Punktfunk's decoded PCM/jitter/AV-sync plumbing, and intentionally returns "unsupported" for microphone capture.
- `patches/pad_audio-embedded.rs` disables the PipeWire-only DualSense speaker/haptics-audio renderer while leaving ordinary SDL controller input and rumble in place.

The build also removes the optional Rust `pipewire` dependency from `pf-client-core`'s desktop feature. This avoids binding the build to desktop libspa headers and matches SpruceOS' ALSA audio stack.

These substitutions do **not** change the Punktfunk wire protocol, trust model, host orchestration, video decode or Vulkan presenter.

## SpruceOS audio integration

At launch, `runtime-env.sh` runs:

```sh
/mnt/SDCARD/spruce/scripts/asound-setup.sh "$HOME"
```

when available. Current Smart Pro S firmware writes a `.asoundrc` containing the device's `audiocodec` speaker route and changes `pcm.!default` to the Bluetooth route when appropriate. The embedded backend therefore opens ALSA device `default` rather than hard-coding a card number.

## Generated provenance

Every CI package contains `punktfunk/SOURCES.txt`; CI also publishes `build-info.json` and `SHA256SUMS` alongside `punktfunk.zip`.
