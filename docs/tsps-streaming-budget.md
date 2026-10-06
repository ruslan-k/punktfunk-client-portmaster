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


### VE clock does not set the decode time

Four device runs at 1280x720@60 with the gate on, each `VConfig.nVeFreq` value
confirmed by reading `ve/clk_rate` from debugfs during the stream:

| `nVeFreq` | measured `ve` | decode mean |
| --- | --- | --- |
| 0 (SoC default) | 576 MHz | 4.565 ms |
| 0 (repeat) | 576 MHz | 4.538 ms |
| 696 | 696 MHz | 4.430 ms |
| 696 (repeat) | 696 MHz | 4.449 ms |
| 1152 | 1152 MHz | 4.548 ms |

Doubling the encoder clock changed nothing measurable, so the vendor wait is not
VE-clock-bound; the small 696 MHz offset (~0.12 ms) is not monotonic and is not
worth a power cost. `PUNKTFUNK_CEDAR_VE_FREQ` stays an opt-in diagnostic and the
shipped default stays 0.

### The exported frames are importable dma-bufs

Read-only device evidence for a zero-copy presenter:

- Every decoded picture slot carries a stable descriptor: `fd = slot + 39`,
  unchanged across thousands of frames (14 slots).
- `/sys/kernel/debug/dma_buf/bufinfo` lists 14 buffers of **1384448 bytes**
  (1382400 + 2 KiB) attached to `1c0e000.ve`; 1382400 is exactly 1280x720 YV12.
- The vendor reports `e_pixel_format = 4` (`YV12`, plane order Y, V, U) with
  `lineStride = 1280` and zero crop.

`PUNKTFUNK_CEDAR_DMABUF_PROBE=1` maps each descriptor once, compares it against
the copy this rung just produced at the standard `YV12` offsets, and logs the
result. It is read-only and never selects the frame path; the plan is to prove
the layout on the device before any presenter import is written.


### The probe result: the exported frames are exactly our copy

CI build `3c1fc7d`, `PUNKTFUNK_CEDAR_DMABUF_PROBE=1`, 60-second run at 720p60 —
all 14 picture descriptors reported:

```
fd=39..52 len=1384448 expected=Some(1382400) offsets=(0, 921600, 1152000)
copied=1382400 matches=true first_difference=None
```

The descriptor is 1382400 bytes of the same bytes the CPU copy produced, plus a
2 KiB tail, at tightly packed `YV12` offsets. The frame path and the numbers were
unchanged by the probe (decode 4.56 ms, lag 0, 0 errors, 60 FPS).

That settles layout, not the presenter. The current importer takes the
two-plane `NV12`/`P010` model only, so a Cedar zero-copy frame needs either a
three-plane planar import or an `NV12` output. `PUNKTFUNK_CEDAR_PIXFMT` requests
`VConfig.eOutputPixelFormat` (1 = planar 420, 6 = `NV12`) as its own axis; the
FBM reports the format it actually delivers (`e_pixel_format = 4` today even
though 1 is requested), so the request has to be measured, not assumed.


### The vendor pixel-format request is ignored

`PUNKTFUNK_CEDAR_PIXFMT=6` (asking `VConfig.eOutputPixelFormat` for `NV12`) was
measured on the device: the FBM still delivered `e_pixel_format = 4` (`YV12`),
the descriptor was still 1384448 bytes of packed `YV12`, the probe still matched
at `(0, 921600, 1152000)`, and decode stayed at 4.478 ms with the same 60 FPS,
lag 0 and zero errors.

So the delivered picture format is not selectable through `VConfig` on this
decoder path. The zero-copy win therefore requires the presenter to accept a
three-plane planar frame (three `R8` images at these offsets, `LINEAR`, plus the
CSC change), because core Vulkan has no three-plane `YV12` image format and the
current importer takes two-plane `NV12`/`P010`/`NV24` only. That is a change to
the working render path, not to the decoder, and is deliberately left as a
separate piece of work: the measured decode is 4.55 ms with the copy included.

### Where the optimisation work lands

| axis | verdict |
| --- | --- |
| output-hold gate (`VConfig+192`) | taken: lag 2 -> 0, decode ~106 -> ~5 ms |
| single final I420 allocation + one pass per plane | taken: copy 3.6 -> 0.91 ms, decode 5.40 -> 4.55 ms |
| frame-package submit | retired: within noise after the gate |
| near-zero poll budget | retired: within noise after the gate |
| VE clock (`nVeFreq`) | closed: 2x clock changed nothing |
| `bNoBFrames` / `smooth` / `display` / `drop_b_delay` | not pursued: the gate already removed the reorder delay |
| vendor `NV12` output | closed: the request is ignored |
| three-plane dma-buf import | open, de-risked: layout proven byte-for-byte, presenter is the work |


### The 720p60 floor, and why sub-3 ms is not reachable here

Correct accounting of the decode stage (the profile's stage 2 wraps the planner
and the feed as well, so they must be subtracted before reading the drain):

| part | 720p60 | 360p60 |
| --- | --- | --- |
| vendor `FRAME_DECODED` | 2822 us | 815 us |
| picture copy | 899 us | 230 us |
| planner | 357 us | 316 us |
| vendor `NO_BITSTREAM` x2 | 42 us | — |
| drain-loop tail (FIFO/PTS/bookkeeping) | ~352 us | — |
| **HUD decode** | **4.61 ms** | **1.81 ms** |

Four measurements decide that the first row is a hardware floor:

1. It scales with pixel count: 4x fewer pixels gave 3.4x less time.
2. It ignores the VE clock: 576 -> 1152 MHz changed nothing.
3. It ignores memory contention: adding a full extra frame copy (2.76 MB of
   traffic per frame) left it at 2718 us against 2770 us.
4. It is not a blocking syscall: `strace -f -T -e trace=ioctl` over 40 s and
   485719 ioctls recorded **no ioctl over 0.5 ms** — there is no wait to shorten.
5. The memory controller is already at its fastest operating point:
   `3120000.dmcfreq` reads 1200000000 with governor `performance` out of
   150/480/800/1200 MHz, so there is no memory OPP left to raise either.

Every lever that could move that number has now been measured and closed, so the
hardware wait is the device's throughput at 1280x720 and nothing in the client
can shorten it.

So the reachable floor is `vendor 2.822 + NO_BITSTREAM 0.042 ~= 2.86 ms`, and it
assumes our own per-frame overhead drops to zero. The removable client-side work
is 0.899 + 0.357 + 0.352 = 1.61 ms, which lands the realistic result at
**~3.1-3.4 ms**, not below 3: a zero-copy presenter alone gives about 3.6-3.7 ms.
Sub-3 ms at 1280x720 on this VE would need the hardware wait itself to fall,
and no lever for that was found.

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
