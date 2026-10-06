# TSPS streaming: ALSA route and software-decoder budget

## Observed baseline

The installed 720p60 software H.264 client logged `Unknown PCM Playback` and
`snd_pcm_open(default): No such file or directory`. Spruce generated
`pcm.spruce_speaker` pointing at `Playback`, but the firmware config did not
provide that alias. Repair only the port-local configuration with a plug->dmix
alias when ALSA confirms it is absent. Keep Bluetooth default and mixer intact.

During moving content the client reported receive backlog flushes (queue depths
6-11 frames), decode-related ABR backoffs, and decode-inclusive delays up to
166 ms. The host remained near 60 FPS with approximately 2 ms encoding. During
lighter content the same client showed 60 FPS and ~14 ms decode-inclusive delay.
This establishes a workload-dependent client throughput boundary, not a constant
host encoder failure. Decode-inclusive latency includes queuing; it is not a
CPU-only decoder benchmark. CPU4 was at 1992 MHz in 199/200 samples, so simply
forcing a governor was not justified. The pinned openh264-sys2 0.9.8 already
builds ARM64 NEON; its decoder thread option carries a segfault warning and is
not enabled speculatively.

## Conservative control

New installations begin at 1280x720, H.264, 30 FPS, 4 Mbps with automatic rate.
This preserves native resolution and doubles the frame budget. It is an explicit
throughput mitigation, **not an optimized or verified 720p60 path**. Existing
settings and host presets are untouched; the UI may select 60 FPS again.

## Acceptance

Use the same moving scene as baseline, verify negotiated 720p30, presented FPS,
backlog flush/keyframe counts, ALSA playback-ready and hardware running state,
then verify audible sound and clean return to Spruce. Unit tests and successful
CI do not prove the physical acceptance criteria. Record device results below
only after the fresh test.

## Native Cedar hardware decode (pin-only rung)

The port installs a fifth decoder rung, `native-cedar`, and config.env selects
it by default. It `dlopen`s the device's vendor decoder stack
(`/usr/lib/libvdecoder.so` and friends) at session start, registers the
plugin, decodes H.264 in hardware, and copies each `VideoPicture` into the
packed-I420 `CpuPlanarFrame` the software rung already feeds. The pin is
`PUNKTFUNK_DECODER=native-cedar`; `auto` never reaches the rung, and any init
failure or decode-error streak falls down the standard ladder — software
included — so a device without the vendor stack runs exactly as before.

Evidence boundary: `cedar:`-targeted logs name the load, init, first picture,
format/stride/offsets and a periodic cadence (`aus`, `frames`, `empties`,
`errors`). A picture arriving in a `VideoPicture` format outside planar
`YUV_PLANER_420`/`YV12`/`NV12`/`NV21` refuses (typed error) rather than
guessing. The `stats:` decode tag for its frames is `native-cedar`. The
zero-copy follow-up (`VideoPicture.nBufFd` → presenter dmabuf import) is
deliberately out of scope for this first stream; timing, frame order and
colour are proven on the copy path first.

Acceptance for the rung, beyond the CI build: menu-launched 720p30 stream with
`cedar: first hardware frame delivered`, non-zero `frames` cadence, colours
matching the software run on the same scene, and a clean demotion if the
vendor stack is absent (`native Cedar init failed — demoting to the standard
ladder`).

### Device ABI (A523)

The device's `VConfig` is **216 bytes** and carries three more fields between
`bGpuBufValid` and `nAlignStride` than the H6-CedarC transcription the rung
started from. The layout was pinned by disassembling the device's own
`/usr/bin/vdecoderDemo` (its store offsets and the memset size): holding
counts at `0x68..0x74`, `memops` at `0x80`, `nVeFreq` at `0xA4`. The same
demo's FBM create line prints `nAlignStride = 0`; with the H6 offsets the
rung's palloc write landed on the device's `nAlignStride` (printed as `1`)
and `DecodeVideoStream` answered `NO_FRAME_BUFFER` forever — zero pictures,
SBM filled, then the healthy software demotion. The rung now drives exactly
the demo-validated values (planar-420 output — the demo prints
`eOutputPixelFormat = 1` — and holding 2/2/2; every other knob zeroed), and
the module's unit tests pin the layout, so a drift fails at test time, not at
the first stream.
