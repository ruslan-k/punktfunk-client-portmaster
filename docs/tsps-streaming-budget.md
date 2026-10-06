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

## First-run defaults follow the decode path

New installations request 1280x720, H.264, 4 Mbps with automatic rate. The
refresh follows the configured decoder: 60 FPS on `native-cedar` (verified
below), 30 FPS on the software rung, which cannot hold that cadence. Existing
settings and host presets are untouched, and the UI can still select either.

The 30 FPS figure began as a mitigation for the software-decoder backlog; it is
kept only for that rung, because the measurement that justified it (queued
decode-inclusive delay under moving content) applies to software decode.

## 720p60 on the verified hardware path

Quiet 120-second run through the shipped runtime defaults, native Cedar, no
diagnostic overrides (build 9badbe4 plus the 60 FPS client setting):

- Negotiated `1280x720@60`; received/decoded/presented medians **60.0 FPS**.
- Sample-weighted mean decode **5.402 ms**; capture-to-presentation **13.149 ms**;
  display 2.597 ms.
- 5580 AUs and 5580 output pictures; 5580 exact original-PTS matches, 0
  unmatched, 0 outstanding, 0 decode errors, lag census all zero.
- Lost 0; skipped peaked at 3 in one window; `session_exit=0`.
- Real DRM frame showed `native-cedar` at `1280x720@60`, no artefacts.

Boundary: after roughly 85 s the activity on the host increased and the received
rate settled at about 39-40 FPS. Client evidence rules out the handheld as the
cause: decode 4.53 ms, display 2.65 ms, queue 0, lost 0, skipped 0, and host
encode 2.8-3.05 ms per frame. The host produced fewer frames per second at the
same per-frame size, so that tail is a host render/capture rate, not a decode or
presentation limit. Report the received rate separately from the negotiated
refresh; a 60 Hz request does not guarantee 60 produced frames.


Use the same moving scene as baseline, verify the negotiated refresh, presented FPS,
backlog flush/keyframe counts, ALSA playback-ready and hardware running state,
then verify audible sound and clean return to Spruce. Unit tests and successful
CI do not prove the physical acceptance criteria. Record device results below
only after the fresh test.


## Packed copy and the remaining decode budget

CI build `9e1e152` (single final I420 allocation, one `copy_nonoverlapping` per
plane) measured on the device at 1280x720@60, shipped runtime defaults:

- decode mean **4.551 ms** (clean run) / 4.617 ms (phase-profiled run) versus
  5.402 ms on the previous build; capture-to-presentation 12.276 ms.
- 6920 AUs, 6920 frames, 6920 exact PTS matches, 0 unmatched, 0 errors, lag
  census all zero, 60.04 FPS received/decoded/presented, `lost 0`.

Opt-in phase census (`PUNKTFUNK_CEDAR_PROFILE=1`, 57 windows, 6840 pictures):

| stage | mean |
| --- | --- |
| 0 planner | 361 us |
| 1 feed AU | 51 us |
| 2 whole decode call | 4572 us |
| 3 RequestPicture | 6 us |
| 4 picture copy | **906 us** |
| 5 ReturnPicture | 8 us |
| vendor FRAME_DECODED | 2846 us |
| vendor NO_BITSTREAM (2.1/AU) | 27 us |

The copy dropped from about 3.6 ms to 0.91 ms, so the hardware wait is now the
largest single item. A single `memcpy` of 1.382 MB in 0.9 ms is about 1.5 GB/s,
which is what reading the vendor's frame buffer costs here; the remaining win is
the DMA-BUF hand-off, not more copy tuning.

Two one-axis candidates changed nothing measurable at this point:
`PUNKTFUNK_CEDAR_FRAME_PACKAGE=1` (4.546 ms) and a 1 us poll budget (4.532 ms).
With the output gate in place, complete-AU submits, frame packages and polling no
longer move the number; all three kept lag 0 and 60 FPS.

## VE clock

`cedarc` on this SoC logs `ve_default_freq = 576` and calls `VeSetSpeed` with
576 MHz; debugfs reports `ve` = 576 MHz under `pll-ve` = 1152 MHz, and the debugfs
`clk_rate` file is read-only (writes are refused), so the clock cannot be probed
by hand. `PUNKTFUNK_CEDAR_VE_FREQ` writes `VConfig.nVeFreq` (the vendor's MHz
unit) and exists only for a controlled one-axis experiment; 0 keeps the SoC
default and is the shipped value. The client never raises it unasked.

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

Acceptance for the rung, beyond the CI build: menu-launched 720p stream with
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
