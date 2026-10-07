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
6. Waiting happens in no syscall at all: tracing `read`, `poll`, `epoll_wait`,
   `futex`, `clock_nanosleep` and friends over a whole run (13051 calls) shows the
   only multi-millisecond call is the session thread's own `epoll_pwait` idle.
   Nothing blocks for anything like 2.8 ms per frame, so the vendor is spinning
   in user space, not sleeping in the kernel.
7. The bitstream is not the cost either: dropping the requested bitrate from
   4000 to 1200 kbit/s moved the vendor wait from 2833 to 2689 us - 144 us, 5%,
   for a visible quality loss.

Every lever that could move that number has now been measured and closed, so the
hardware wait is the device's throughput at 1280x720 and nothing in the client
can shorten it.

So the reachable floor is `vendor 2.822 + NO_BITSTREAM 0.042 ~= 2.86 ms`, and it
assumes our own per-frame overhead drops to zero. The removable client-side work
is 0.899 + 0.357 + 0.352 = 1.61 ms, which lands the realistic result at
**~3.1-3.4 ms**, not below 3: a zero-copy presenter alone gives about 3.6-3.7 ms.
Sub-3 ms at 1280x720 on this VE would need the hardware wait itself to fall,
and no lever for that was found.


### Both "below 3 ms" suggestions, measured and rejected

The two levers that could shrink the hardware wait without touching the client
were tested on the device.

**Turning deblocking off is not available.** x264's `--no-deblock` only zeroes the
filter strengths; `disable_deblocking_filter_idc = 1`, which is what makes a
decoder skip the in-loop filter, is not something the encoder signals. And the
compute angle is already weak evidence: a 3.3x bitrate cut moved the wait by 5%.

**Lowering the resolution to 1152x648 works on the vendor side but loses overall.**

| | 1280x720 | 1152x648 |
| --- | --- | --- |
| vendor `FRAME_DECODED` | 2836 us | **2100 us** |
| picture copy | 876 us | **1523 us** |
| planner | 338 us | 340 us |
| HUD decode | 4.576 ms | 4.432 ms |
| capture-to-presentation | **12.36 ms** | **21.72 ms** |

The vendor prediction was right - 736 us off the wait - but the vendor pads the
stride at this width, so the packed copy path stops applying and the row loop
costs 647 us more, which eats the whole gain. Worse, the presenter now has to
scale instead of blit and end-to-end latency nearly doubles. A lower resolution is
therefore not a cheaper decode here, it is a slower stream.


### The bitstream axis, measured to the end

The last suggestion for shrinking the hardware wait was to make the *stream* cheaper
to decode (deblocking off, a faster encoder profile). The vendor's own demo decoder
was timed on identical 720p60 content at one bitrate, encoded different ways
(mean of two interleaved runs, 1200 frames each):

| encoding | ms/frame | vs baseline |
| --- | --- | --- |
| x264 veryfast | 3.719 | — |
| + `no-8x8dct` | 3.717 | 0% |
| + `no-weightb` | 3.644 | -2% |
| + `partitions=none` | 3.782 | +2% |
| + `no-cabac` (CAVLC) | 3.631 | -2% |
| + `ref=1` | 3.721 | 0% |
| + `no-deblock` | 3.648 | -2% |
| **+ `bframes=0`** | **2.711** | **-27%** |
| `ultrafast` (all of them) | 2.461 | -34% |

One tool accounts for almost all of it: **B-frames**. Every other coding tool is
worth 0-6%, which also retires the deblocking suggestion - it is 2% here.

And that headroom does not exist in production: the host documents "Low-latency
preset, B-frames off" (`crates/pf-encode/src/lib.rs`), and the vendor output-hold
gate only arms after the planner confirms the live stream has no B slices, which it
does. The production stream is therefore already the cheap variant: the client
measures 2.822 ms per frame on it, against 2.711 ms for the B-less clip here.

A clip comparison can only mislead if it is not checked against the live stream:
this one first looked like a 27% win and turned out to describe a stream the host
never sends.


### Client-side work, first item: a cheap planner path (landed, verified)

An ordinary P AU carries no colour, recovery or shape state: its only facts are the
slice types and the frame number. `patches/cedar_fast_au.rs` reads exactly those
from the Annex-B NALs - refusing every AU that contains an SPS, PPS, SEI, IDR,
partition NAL, an unparseable slice header or slices that disagree on `frame_num`,
and any AU with no VCL NAL at all - and the full planner still runs for all of
them, refreshing what the cheap path needs. `PUNKTFUNK_CEDAR_FAST_PLAN=0` restores
the old behaviour.

The scanner is verified against the planner as an oracle on the live stream:
`PUNKTFUNK_CEDAR_FAST_VERIFY=1` runs both, ships the planner's answer, and counts
disagreements. On a 720p60 run: **2400+ AUs compared, 0 mismatches**.

Device A/B, same content, profiled:

| | planner stage | decode | capture-to-presentation |
| --- | --- | --- | --- |
| fast path on | **69 us** | **4.279 ms** | 12.098 ms |
| `FAST_PLAN=0` | 356 us | 4.543 ms | 12.381 ms |

That is **-0.264 ms** on decode, with 60 FPS, zero lag, zero errors and every PTS
matched in both runs. The planner stage itself falls 356 -> 69 us, so the mechanism
is the one claimed, not a run-to-run artefact.

### The drain tail: measured, not yet attributed

Stage 6/7/8/9 instrumentation splits the drain loop: the frame arm is 913 us per
call (of which `take_picture` - RequestPicture, copy, ReturnPicture - is 909 us),
the non-frame arm 0.4 us, and the FIFO/ledger work inside the frame arm about 4 us.
That accounts for the loop's arms, yet about **330-350 us per AU** of the drain
remains outside every arm, and the session's own independent timer sees it too, so
it is real rather than a profiling artefact.

It is not the retry sleep (`POLL_US=0` measured 4.300 ms against 4.258 ms), not the
profiler (unprofiled 4.333 ms against profiled 4.293 ms) and not the cost of reading
the clock (about 1.3 us per call on this device). Nothing in the loop's visible code
explains it, so it stays open: claiming a fix here would be a guess.


### Zero-copy on this device: the import is refused, measured

First device run of `PUNKTFUNK_CEDAR_ZEROCOPY=1` (build 5ff81cc): the picture
appeared, and it was NOT zero-copy. The presenter refused the chroma plane and the
client demoted, safely:

```
WARN presenter: hardware present failed error=plane 1: create 640x360 R8_UNORM
     image (modifier 0x0000000000000000): ERROR_INVALID_DRM_FORMAT_MODIFIER
WARN presenter: demoting the decoder to software
stats: "decoder":"software"  e2e mean 89-167 ms, 54 fps
```

So `vkCreateImage` accepts the luma plane (1280x720 R8_UNORM, LINEAR) and refuses
the 640x360 chroma plane with the same format and modifier. The rung's
`modifier_importable()` query answered yes for the pair, which is why the failure
surfaced at create rather than as a clean refusal - the query is not sufficient
here, and a driver that refuses create after answering the query is exactly the
case the demotion ladder exists for.

A device-side probe with a device created (so the extension is actually enabled)
was run to name the tuple. Its answer - "the driver ACCEPTS every combination
tried" - does not hold: the probe built its images with `VkFormat` 1000014000
(`VK_FORMAT_R8_UNORM` is 9) and `VkImageDrmFormatModifierExplicitCreateInfoEXT`
with sType 1000158001 (the header and ash 0.38 both say 1000158004), so the
driver never received the plane layout the presenter sends. Re-run with the real
constants, the refusal reproduces outside the presenter - see "Zero-copy: the
plane offset belongs to the binding" below.

The device extension census: VK_EXT_external_memory_dma_buf, VK_KHR_external_memory_fd
and VK_EXT_image_drm_format_modifier are all present; VK_KHR_queue_family_foreign is
absent (ARM exposes VK_EXT_queue_family_foreign instead). The presenter's setup log
does NOT print "device lacks the dmabuf import extensions", so its extension set was
enabled and this is not the gate either.

What remains is the exact parameter tuple the presenter passes. The next build names
it: the import failure now carries offset and stride alongside the extent, format and
modifier, because ERROR_INVALID_DRM_FORMAT_MODIFIER_PLANE_LAYOUT_EXT is a complaint
about the LAYOUT and the extent alone does not identify it.

Original note, kept because it is still the reason a plain probe misleads:
(a plain `vkGetPhysicalDeviceFormatProperties2` probe reports zero modifiers
because `VK_EXT_image_drm_format_modifier` is not enabled without a device):
whether any (format, extent, modifier) combination imports these planes, or
whether a single linear dma-buf with three R8 planes is simply not importable on
this Mali stack. Until that is answered, zero-copy stays opt-in and unclaimed.

### Zero-copy: the client half, and what it costs

`PUNKTFUNK_CEDAR_ZEROCOPY=1` makes the rung hand the presenter the vendor's own
dma-buf instead of a CPU copy: `hold_picture` builds a `DmabufFrame` from
`n_buf_fd` plus the YV12 offsets the DMA-BUF probe already proved byte-for-byte
(Y at 0, V at `luma_len`, U at `luma_len + chroma_len`, so the plane list carries
Y, Cb, Cr in the order the planar CSC binds), and it does NOT call `ReturnPicture`.
The picture stays held until the presenter's fence, because the vendor would
otherwise write over a surface the GPU is still sampling.

Releases come back over a channel and are drained on the decoder thread before
every vendor call (`drain_releases`), since the vendor stack is single-threaded.
Held pictures occupy slots in a finite vendor pool, so the drain has to keep up:
`held`/`held_peak`/`released` are logged for exactly that check.

Two honest limits, both unmeasured on device so far:

* **The intra-refresh mark is lost.** This rung derives `LocalRecovery` and the
  copy path carries it on `CpuPlanarFrame::recovery`. `DmabufFrame` has no such
  field, so a zero-copy frame reports `references_clean: false` - no evidence,
  rather than a guessed one. Carrying it properly means adding a field to
  `DmabufFrame` and updating the VAAPI/V4L2 constructors.
* **Pool starvation is untested.** If the presenter lags, `RequestPicture` can
  return null while releases are still in flight; the rung treats that as "no
  picture" and the retry budget bounds the drain.

Default off: the copy path is unchanged unless the knob is set.

### Zero-copy: the shape the presenter change must have

The presenter already owns everything the colour path needs - `CscPass::new_planar`
builds the three-binding planar pass, `bind_planes_planar` binds Y, Cb, Cr, and the
CPU I420 rung proves both on this device - so the missing piece is only the import.

It has to go through the existing cached importer, and that is not obvious from the
outside. `dmabuf::get_or_import` takes the frame **by value** (that is how the
decoder's `DrmFrameGuard` reaches `HwFrame::_guard`; `FrameGuard` is `pub(crate)` in
`pf-client-core`, so the presenter can neither name nor construct one) and it caches
imported images per `pool_key` - one import per picture slot, not per frame, because
an import costs image creates, memory imports and mappings.

A first attempt bolted a separate `import_planar` beside that function. It compiled
in the client crate but not in the presenter (two errors: `FrameGuard` not in scope,
and a tuple struct with private fields), and more importantly it would have
re-imported three images **every frame** - more expensive than the 0.9 ms copy it
removes. It was reverted rather than patched over.

The change that does work generalises `HwFrame`/`Planes` from `[_; 2]` to a plane
list plus a `planar` flag, keeps the per-`pool_key` cache, and picks the CSC pass by
that flag. The client half is already in place and tested: `CedarFrameGuard` returns
the vendor picture to the decoder thread over a channel when the presenter drops it
after its fence, and `FrameGuard::Cedar` is wired into the enum by the patcher.

Not done, and not claimed: the lane wiring, the frame construction in the Cedar rung
behind an opt-in knob, the release drain on the decoder thread, and the device test.

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

## Zero-copy: the plane offset belongs to the binding (measured)

The presenter's planar import was refused on the chroma plane with
`ERROR_INVALID_DRM_FORMAT_MODIFIER_PLANE_LAYOUT_EXT`. A device probe with a device
created and valid constants (`R8_UNORM` = 9, explicit-create sType 1000158004,
modifier 0) reproduces that outside the presenter, and one axis at a time gives the
rule:

| plane layout | result |
| --- | --- |
| 1280x720, 640x360, 640x368, 640x364, 512x512, 256x256, 64x64 at offset 0 | SUCCESS |
| any non-zero offset (4096, 65536, 230400, 460800, 921600, 1152000, 1155072) | refused |
| stride below the plane width (512 for a 640-wide plane) | refused |
| stride at or above the width (640, 1024) | SUCCESS |
| usage SAMPLED, TRANSFER_DST, both | SUCCESS |
| `R8_UINT`, `R8G8B8A8_UNORM` | refused |
| modifier LINEAR (0) | SUCCESS |
| modifier AFBC (`0x0100_0000_0000_0000`), INVALID | refused |

So the extent, the usage and the offset's alignment are not the gate: **the explicit
plane offset must be 0**, and the plane's byte offset belongs to `vkBindImageMemory`.
The same probe measured the shape the presenter now uses:

- three images (Y 1280x720, U and V 640x360) with layout offset 0 and stride equal to
  the plane width: all SUCCESS, `memoryRequirements` size 921600 / 230400, alignment 64;
- one imported `VkDeviceMemory` from the picture fd, allocation size 1382400, no
  dedicated block: SUCCESS, and binding those images at 0 / 1152000 / 921600: SUCCESS.
  Every offset is a multiple of the 64-byte alignment, and 1152000 + 230400 = 1382400
  is exactly the export's payload;
- a dedicated allocation bound at a non-zero offset is accepted by this driver too,
  but the spec requires `memoryOffset = 0` for a dedicated allocation, so the import
  no longer uses one.

`vkCreateImage` with `VK_IMAGE_TILING_LINEAR` and a `DMA_BUF_EXT` chain is refused for
every plane, including 1280x720 at offset 0, so a "plain LINEAR external image" is not
an alternative to the modifier chain here - it does not create at all. A local
(non-external) LINEAR image of the same extent does create, which is what the removed
diagnostic line measured and why its result was misread as a positive.

Proven: creates, imports and binds. Not proven: that the sampled picture is correct,
which the device run has to show.

The port change is one line of meaning in `scripts/patch-presenter-planar.py`:
`plane_image` writes offset 0, allocates `offset + reqs.size` bytes from the plane fd
and binds at the plane offset. The diagnostic retries (plain LINEAR, sized layout)
went with it; they answered a question that is now answered.

### The external-VkBuffer route, checked because it was proposed

A review suggested giving up on `VkImage` and importing the whole 1382400-byte export
as one `VkBuffer` read through an `R8_UNORM` uniform texel buffer. Measured on the
device:

- `vkGetPhysicalDeviceExternalBufferProperties(DMA_BUF_EXT)` answers `features = 0x0`
  for every usage tried (TRANSFER_DST, UNIFORM_TEXEL, STORAGE_TEXEL, UNIFORM_BUFFER,
  STORAGE_BUFFER, VERTEX) - "not importable" - and the import works anyway:
  `vkCreateBuffer` with `VkExternalMemoryBufferCreateInfo(DMA_BUF_EXT)`,
  `vkGetMemoryFdPropertiesKHR` (`typeBits = 0x2`), `vkAllocateMemory` with
  `VkImportMemoryFdInfoKHR` and `vkBindBufferMemory` are VK_SUCCESS on a real dma-heap
  buffer. That capability query cannot be used as a gate on this driver.
- The limits do not block it either: `maxTexelBufferElements` and
  `maxStorageBufferRange` are both 268435456 (2^28) and `maxUniformBufferRange` is
  65536, so a 1382400-element texel buffer is legal.

The route works, and it is still not the one taken: it needs a CSC shader over
`texelFetch` and its own descriptor set, while the image path keeps the existing
planar CSC pass and costs one line in the create info.

Probe note for the next ctypes run on this device: the driver pads
`VkPhysicalDeviceLimits` to offset 296 inside `VkPhysicalDeviceProperties` (a struct
pre-filled with 0xAA comes back written at 0..291 and 296..819). Read at the spec's
292 and every limit is shifted by one `uint32`, which reads as nonsense
(`maxImageDimension1D` = 0, `maxPushConstantsSize` = 268435456).

### Zero-copy on the device: the import works

Build `2aacc46` installed on TSPS (`bin/punktfunk-session` sha256
`9597b1796b924853ffda57de5b1a0832fb54185235514d3c39a1b650de0e41f6`, `bin/punktfunk`
unchanged at `94cc24c26736e3963b82d28895d95d114ac244dfb7c37a200318c89f0abfd69b`,
backup `/mnt/UDISK/punktfunk-backup-20261007-044317.tar.gz`), launched through the
ordinary Spruce principal flow, 90 s, 720p60:

- `decoder=native-cedar` throughout, no `hardware present failed`, no demotion to
  software, `session_exit=0`, `lost 0`, `skipped 0`, lag census all zero.
- The zero-copy handover ran: 4221 `picture handed to the presenter as a dma-buf`
  lines, and the release counter kept up — `released=4200 peak_held=3 still_held=1`
  at the end. No `NO_FRAME_BUFFER`, no `ReturnPicture refused`.
- A real KMS capture (`kmsgrab`) mid-stream shows a correct picture with correct
  colours, so the three planes are read from the right offsets and the CSC order
  (Y, Cb, Cr) is right. The chroma planes are not swapped or mirrored.

One-axis A/B on the same scene, same day, both with `PUNKTFUNK_CEDAR_PROFILE=1`:

| | copy (default) | `PUNKTFUNK_CEDAR_ZEROCOPY=1` |
| --- | --- | --- |
| stage 2 whole decode call | 3363 us | **2578 us** |
| stage 4 picture copy | 731 us | **0 (not taken)** |
| stage 7 drain frame arm | 3277 us | **2494 us** |
| decode-inclusive (receive -> decoded) | 46.6 ms | 45.5 ms |
| capture-to-presentation | 61.6 ms | 59.1 ms |
| received / decoded / presented | 60.0 / 60.0 / 60.0 FPS | 60.0 / 60.0 / 60.0 FPS |

So the copy is gone and the decode call is ~0.8 ms cheaper, which is a little more
than the 0.9 ms the copy itself cost.

### The ~43 ms in front of the decoder: measured, then explained

The numbers above are 5x the latency this document recorded earlier (decode 4.55 ms,
capture-to-presentation 12.1 ms), and the gap is not in the decode work: the census
puts the whole decode call at 2.6-3.4 ms while the client's decode-inclusive metric
(receive -> decoded) sits at ~46 ms. That is a queue of about 2.5 frames in front of
the decoder.

It is not this change. Three controls, all at ~46.6 ms decode-inclusive:

- the copy path with the previous presenter binary (`bin/punktfunk-session` sha256
  `8a5f0e48...`, the `9e66880` build) restored for one run: 46.6 ms, stage 2 3362 us;
- the client's UDP receive buffer is not the cause: raising `net.core.rmem_max` from
  212992 to 4194304 changed the same run's number from 46.6 to 46.6 ms (the setting was
  reverted afterwards);
- `bin/punktfunk` is byte-identical to the pre-change install, so the client's receive
  and decode scheduling is unchanged.

That left the consumer side as the suspect, and it was wrong - see "The ~43 ms was the
wrapper" at the end of this document. No queue-telemetry build is needed for this
question.

#### Three cheap hypotheses, measured and dropped

The ~43 ms sits in front of the decoder, so three candidates were run on the
installed build (`2aacc46`, no rebuild) before writing any telemetry:

- **Host send cadence.** `PUNKTFUNK_PERF=1` prints the client's own AU inter-arrival
  window (`frame inter-arrival jitter (window)`, `client/pump/data.rs`). Twelve
  consecutive windows: `arrival_p50_us` 16733-16814, `arrival_p95_us` 16903-17433,
  `arrival_max_us` <= 19373, `late=0`. Delivery is one frame every 16.75 ms with no
  clumps, so the queue is not a burst the channel is faithfully holding.
- **Thread priorities.** Every hot thread reports `priority raised` via `setpriority`
  (`decode`, `core-pump`, `presenter`, `frame-wake`, audio) and there are no
  `priority refused` lines. `PUNKTFUNK_THREAD_BOOST=0` for one run changed nothing:
  decode-inclusive 46.6 ms, capture-to-presentation 61.4 ms, against 46.6 / 61.6 with
  the boost on. No priority interaction to find.
- **A real backlog.** No `jump-to-live`, no flush and no shed line appears in any run,
  so the standing depth never reaches `QUEUE_HIGH` (6). Depth itself was inferred from
  the latency, never measured, and nothing retains two AUs on purpose: `QUEUE_LOW` is
  hysteresis inside `JumpToLive::observe`, and the H.264 path pops the front
  unconditionally.

That leaves the consumer side: with arrival clean and the decode 2.6 ms of a 16.7 ms
frame, something downstream of the decoder keeps the channel from draining. The
measurement that separates the remaining candidates is the one proposed for the next
build - timestamps at receive, channel pop, decode enter, decode exit and hand-off,
plus a queue-depth histogram - and it should include the hand-off into the presenter's
bounded channel, because a 60 Hz FIFO presenter is the only part of the path that has
been shown to throttle a 60 fps producer.

### The ~43 ms was the wrapper, not the client

Every run in the two sections above was launched by the diagnostic harness
(`/mnt/SDCARD/Roms/PORTS/cedar-test.sh`, written by the measurement script), which
sourced `runtime-env.sh` and then **cleared** `PUNKTFUNK_CEDAR_LOW_DELAY` and
`PUNKTFUNK_CEDAR_POLL_US`. Those two are not diagnostics: `runtime-env.sh` ships them
as the port's defaults (`auto` and `5000`), so every A/B above ran without the
low-delay configuration the port actually installs.

The same build, launched the way the menu launches it (`Punktfunk ZeroCopy` ->
`Punktfunk.sh` -> `launcher-common.sh`, with `PUNKTFUNK_CEDAR_LOW_DELAY=auto`,
`PUNKTFUNK_CEDAR_POLL_US=5000` and `PUNKTFUNK_CEDAR_ZEROCOPY=1`), streamed 720p60 for
eight minutes:

| | harness runs (LOW_DELAY cleared) | menu launch (shipped defaults) |
| --- | --- | --- |
| decode-inclusive (receive -> decoded) | 45.5-46.6 ms | **2.92 ms** (p95 2.99) |
| capture-to-presentation | 59.1-61.6 ms | **8.98 ms** (p95 9.80) |
| display | 8.3-9.8 ms | **1.16 ms** |
| decoded / presented | 60.0 / 60.0 FPS | 60 / 60 FPS |
| frames | ~4200 per 75 s | 29701, `empties=0 errors=0` |
| lost / skipped | 0 / 0 | 0 / 0 |

Zero-copy was active in that run and it can be shown without debug logging: the session
process holds 72 `dmabuf` file descriptors, which is the presenter's per-slot imports of
the vendor planes (14 FBM slots x 3 planes, plus dups). The presenter's
`native_zero_copy=(0, 0)` counter is about scanout, not about this import, and stays
zero either way - do not read it as "no zero-copy".

So both hypotheses that survived the earlier sections - a FrameChannel backlog and a
stalled hand-off - were wrong. With the shipped defaults the client decodes 720p60 at
2.92 ms with zero-copy on, against 4.55 ms for the copy path measured before it. The
harness A/B still shows the mechanism (removing the copy removes ~0.7-0.8 ms of the
decode call), but it must not be used for absolute latency numbers, because it changes
the configuration under test.

### Zero-copy ships on, with a rung-local fallback

`PUNKTFUNK_CEDAR_ZEROCOPY` defaults to 1 in `runtime-env.sh`, so the ordinary
`Punktfunk` menu entry streams zero-copy; `config.env` documents the knob, and `0`
restores copying. The separate `Punktfunk ZeroCopy` launcher that carried the knob
during development is gone - one menu entry, and it is the real configuration.

A refused import no longer costs the decoder. `pf-client-core` demoted the whole rung
to software when the presenter signalled that it could not display a hardware frame
(`video.rs::force_software`), which is exactly what the first device run did: 720p60
hardware decode fell to software over one refused plane. The Cedar rung can hand
copies instead, so the demotion now asks it first:

- `NativeCedarDecoder::drop_zerocopy()` clears `zerocopy` for the session, logs it, and
  returns `true`; the rung keeps decoding down the packed-copy path it already had.
- `Decoder::drop_zerocopy()` exposes it to the pump (`#[cfg(target_os = "linux")]`, the
  same shape as `poll_cedar_ready`).
- The pump's demotion site calls it before `force_software()`, so only a rung with
  nothing to drop - or a Cedar stack that is itself broken - reaches software.

Proven so far: creates, imports, binds, a correct picture, 29701 frames with
`empties=0 errors=0`, and 2.92 ms decode with zero-copy on. Not yet exercised on the
device: the fallback itself, which needs a run with the import deliberately refused.

### The two gaps, and the hook that makes one of them reachable

`PUNKTFUNK_CEDAR_REFUSE_IMPORT` (presenter, `scripts/patch-presenter-refuse-import.py`)
makes the presenter refuse every imported dma-buf frame - the exact signal the pump
answers with `drop_zerocopy`. It takes the path the presenter already takes when it has
no import support (`force_software.store(true)` then `Ok(false)`), so the pump is not
being driven through a private door. Without the hook the fallback is unreachable on
this device, because the import works.

Two runs close what is left:

1. **Hold and release counters.** `cedar-zerocopy: picture handed to the presenter as a
   dma-buf`, `pictures returned to the vendor` and `peak_held`/`still_held` are
   debug-level and do not print at `RUST_LOG=info`, and the close line does not carry
   them either. They need `RUST_LOG=info,pf_client_core::video_cedar=debug` and no hook.
   At `info` the evidence for zero-copy is indirect but consistent: 72 `dmabuf` file
   descriptors held by the session, `pts_matches=0` (the copy path matches PTS per
   picture), no import failure, no demotion, `output_lag_frames=[0, 0, 0, 0, 0]`.
2. **The fallback itself.** With the hook set, the log must show `cedar: presenter
   refused the imported planes - decoding copies`, the rung must stay `native-cedar`
   with no `demoting to software`, and the decode call should return to the copy-path
   cost (~0.7-0.8 ms more) while the picture stays correct.

### The fallback, verified on the device

`PUNKTFUNK_CEDAR_REFUSE_IMPORT=1` with `RUST_LOG=info,cedar=debug`, build `47c37b9`,
log `punktfunk-20261007-112620-4397`:

1. `picture handed to the presenter as a dma-buf token=1` - zero-copy running.
2. `PUNKTFUNK_CEDAR_REFUSE_IMPORT: refusing an imported dma-buf frame` - the presenter
   refuses.
3. `cedar: presenter refused the imported planes - zero-copy off, decoding copies` - the
   rung drops the hand-off.
4. `first picture decoded (vendor format copied directly to packed I420)` - the rung
   stays on Cedar and switches to the packed copy.
5. No `demoting to software decode` and no `software decoder opened`: the decoder stayed
   `native-cedar` for the whole run, which is the point of the fallback.
6. `decode` 3.26 ms (p50 3.25, p95 3.48) against 2.75 ms with the hand-off - the copy
   cost, as predicted. `lost 0`, `skipped 0`, `empties=0 errors=0`, `pts_matches=3922
   pts_unmatched=0 pts_outstanding=0`, `output_lag_frames=[3922, 0, 0, 0, 0]` (bucket 0
   is the one-in-one-out signature under `LOW_DELAY=auto`; the older copy-path run that
   booked lag in buckets 2-3 had the low-delay mode cleared by the harness).

Three defects had to be fixed to get there, and two were in the test rig rather than the
port:

- the hook sat in the unguarded dmabuf arm, which a device with a working import never
  takes: it installed cleanly, looked right, and never fired;
- `config.env` is *sourced*, not exported, so a knob without a `runtime-env.sh`
  re-export stays a shell variable in the launcher and never reaches the process. Every
  other knob works only because `runtime-env.sh` re-exports it explicitly;
- the fallback itself was half-effective: the presenter signals once per refused frame,
  so the second signal - about a frame already in flight - found nothing left to drop
  and demoted the decoder to software (openh264) anyway. The rung now records that the
  refusal was its own and absorbs later ones, because demoting cannot fix a refusal the
  copies already answer.

The hold and release path is also verified at debug level: 8411 pictures handed over and
8411 returned, `ReturnPicture refused` zero, `peak_held=3`, steady `still_held=1`.

## Drain retry backoff: 200 us -> 50 us (measured, -130 us)

`retry_async` (patches/cedar_async.rs) retries only when no picture has been produced
yet in this drain call, the result is CONTINUE/NO_BITSTREAM and the poll budget has
not run out. The retry sleeps before the poll that starts the vendor decode, so the
sleep lands on the AU->picture path one to one. The phase census showed it as
"unattributed drain body" (296 us/frame against 39 us of arm timers) because it sits
inside stage 6 but outside stages 8 and 9; stage 10 now measures it directly.

Three arms, ~2400 frames each, same pipeline, same stream:

| retry_us | whole decode | stage 6 (body) | stage 10 (backoff) | polls/frame |
|---|---|---|---|---|
| 200 (old default) | 2746 us | 296 us | (not measured) | 1.04 |
| 50 (new default) | 2616 us | 152 us | 112 us | 1.04 |
| 0 (tight poll) | 2633 us | 40 us | 0.1 us | 26.8 |

- `sleep(50 us)` really costs ~107 us on this device (timer/scheduler granularity),
  which is why the old 200 us nominal produced the observed ~257 us.
- 50 and 0 are equal within noise on decode (2616 vs 2633 us): at 50 the sleep roughly
  matches how long the SBM parser needs the AU anyway, so 0 only replaces the sleep
  with ~27 extra NO_BITSTREAM polls (9.4 us each). 50 is the default; 0 buys nothing
  and polls 27x more.
- Quality is untouched: this is a polling backoff, not a bitstream or picture knob.
  Both arms kept native-cedar, zero-copy, lost 0, skipped 0.

### The mid-run step is the network, not the decoder

In the 0 us arm the run changed regime ~65 s in: display 1.1 -> 26.6 ms, e2e
8.8 -> 34.8 ms, stable afterwards, while the decode census stayed identical
(stage 6 39.8 us, stage 10 0.1 us, whole decode 2617 us, vendor unchanged) and every
presenter field stayed identical (submit 71 us, fence 16 us, present 48 us,
jitter 0, late 0, misses 0).

**The first reading of this was wrong and is corrected here.** I first blamed the
client's Wi-Fi, because `quinn_udp: sendmsg error: Os { code: 5, Input/output error }` /
`halting segmentation offload` appeared just before the step. Two later runs disproved
that: those lines are a constant background every ~10 s in *both* power-save states, and
the step itself appeared in exactly one of four arms:

| arm | step? |
|---|---|
| retry 200, power save on | no |
| retry 50, power save on | no |
| **retry 0, power save on** | **yes (display 26.6 / e2e 34.8)** |
| retry 50, power save off | no |

So the step tracks `retry_us=0`, not the link: a tight poll runs ~27 vendor calls per
frame in the drain and the presenter's queue ends up about 1.5 frames deep (its own
durations stay identical - submit 71, fence 16, present 48 - while `display`, which is
arrival to present, grows). That is the second independent reason the default is 50 and
not 0. Turning Wi-Fi power save off showed no measurable benefit either (arrival p50
16.75 ms against 16.73-16.81 with it on), so the device was left stock.

## Host side: the encode clock sag (measured, -1.08 ms end to end)

The client was measured to its floor, so the next step was the host. The host runs on
the same box (minisforum: `punktfunk-host` user unit + a managed nested gamescope on
HDMI-A-1, VAAPI H264 1280x720@60). It has its own per-stage recorder — `punktfunk-host
ctl stats record start|stop`, samples in `~/.config/punktfunk/captures/`, and it needs
no `PUNKTFUNK_PERF` (`let measure = self.perf || self.stats.is_armed()`). Stage names for
the Linux native path, from the code: `queue = delivery->submit`, `capture =
try_latest(ring+convert)`, `submit = encode_picture`, `encode = lock_bitstream(sched+ASIC)`,
`send = pace`, plus `host_p50/p99_us` and `rtt_us`.

First recording closed the whole budget against the client's own stats from the same run:

| stage | ms | share |
|---|---|---|
| host encode | 2.50 | 27% |
| network, one way (rtt 4.54 / 2) | 2.27 | 24% |
| client decode | 2.63 | 28% |
| client display | 1.22 | 13% |
| host submit + send | 0.40 | 4% |
| host queue + capture | ~0 | 0% |
| **sum** | **9.06** | |

The client measured e2e 9.34 ms in that window, so the decomposition closes to 3%.

**The finding: the GPU sat at 400 MHz of 2200 during the stream.** Live: `SCLK 400 MHz`,
`MCLK 800 MHz`, `VCN Load 9%`, `GPU Load 0%`, 7.18 W. The encode is a 2.5 ms burst per
frame, so amdgpu's DPM never ramps. The host has the mechanism for exactly this:
`PUNKTFUNK_PIN_CLOCKS` (`linux/gpuclocks.rs`, "encode clock sag removed", refcounted,
restores the previous level when the last client disconnects). It writes
`power_dpm_force_performance_level`, which normally needs root — here the attribute is
world-writable (666), so the user unit writes it itself.

Applied as `PUNKTFUNK_PIN_CLOCKS=1` in `~/.config/punktfunk/host.env` (the unit's
`EnvironmentFile`), host restarted. Same client build, same stream, same codec and
bitrate — only the clocks changed:

| | unpinned | pinned |
|---|---|---|
| SCLK | 400 MHz | 2200 MHz |
| host encode p50 | 2499 us | **1587 us** (-36%) |
| host_p50 | 2942 us | **2143 us** (-27%) |
| host_p99 | 5620 us | **2663 us** (-53%) |
| client decode median | 2.615 ms | 2.622 ms (unchanged) |
| **client e2e median** | **9.36 ms** | **8.28 ms** (-1.08, -11.5%) |
| rtt | 4.54 ms | 4.40 ms (so the network is not the delta) |

Quality is untouched: same encoder, codec, resolution, bitrate. The pin auto-restored
after the session ("amdgpu performance level restored"), so it only applies while
streaming.

Budget after the pin: host_p50 2.14 + network 2.20 + client decode 2.62 + display 1.17
= 8.13 ms against a measured 8.28 ms. What is left is the VPU floor (2.33 ms), the Wi-Fi
path (power save made no measurable difference) and the host encode (VCN at 20% load, so
it is the ASIC's per-frame latency, not saturation) — further gains there would trade
picture quality, which is out of scope.

## Host video clocks (VCN) under the pin: closed

The pin raised SCLK, but AMD has separate video domains, so the next question was whether
the encoder's own clocks were still sagging. The nodes exist on this box
(`/sys/class/drm/card1/device/pp_dpm_vclk`, `pp_dpm_dclk`), and during a pinned stream
they read:

```
SCLK 2200/2200   VCLK 1440/1440 (level 7 of 8)   DCLK 1028/1028 (level 7 of 8)   VCN load 19-20%
```

Stable across samples, both at their top level. So there is no second clock axis left to
pin: the host encode is already running on full clocks, and VCN at ~20% load says it is
the ASIC's per-frame latency, not saturation. Host encode is closed at ~1.8 ms.

## The budget as the client reports it

The client's own stats window carries the split, so the budget can be closed without
touching the host: `net` (one way), `host_encode`, `decode`, `display`, plus `rtt_us`.
A 25-window run with the pin on:

| window | e2e p50 / p95 / p99 | net p50 | host_encode p50 | decode p50 | display p50 | rtt |
|---|---|---|---|---|---|---|
| 2-24 (median) | 8.25 / 9.02 / 9.35 | 2.30 | 1.85 | 2.58 | 1.15 | 3.8 |

8.25 ms measured against net 2.30 + host_encode 1.85 + decode 2.58 + display 1.15 +
host submit/send ~0.35 = 8.23 ms. The tail is tight: p95 stayed 8.8-9.7 and p99 9.2-11.1
for 23 of 25 windows, with the exceptions being the bring-up window (p99 48 ms) and two
isolated windows (p99 21 ms, and one at p95 18.4). No sustained step.

## Production baseline

Fixed unless a measurement shows a specific problem:

```
1280x720 @ 60
client: native Cedar, LOW_DELAY=auto, POLL_US=5000, RETRY_US=50, FAST_PLAN=1, ZEROCOPY=1
        zero-copy import failure -> Cedar packed copy -> software only on real breakage
host:   PUNKTFUNK_PIN_CLOCKS=1
```

Open (optional, in order): a long soak for the rare tails (p99 spikes of 18-21 ms exist
in isolated windows) with RSSI/retransmit/quinn-error counters alongside e2e; real games
rather than a desktop scene; and only then an allocation-free Cedar cleanup
(`PtsLedger` BTreeMap -> ring, `held` HashMap -> fixed slots, `DmabufFrame.planes`
Vec -> fixed array). The `PUNKTFUNK_CEDAR_REFUSE_IMPORT` hook is not a hot-path cost -
it reads the environment once through a `OnceLock` - so removing it is tidiness, not
performance.

## Allocation-free cleanup: tried, measured slower, reverted

Two per-frame containers were rewritten to remove allocations (`PtsLedger` BTreeMap ->
fixed 128-slot window, `held` HashMap -> short Vec with a linear scan), with the existing
contract tests plus a new one for a window with holes. Both rewrites were correct - 224
windows, lost 0, no pool exhaustion, no `ReturnPicture` refusal - and both were slower:

| arm (equal 25-window samples, runs minutes apart) | decode p50 median | p50 min/max | decode p99 | e2e p50 | actual Mbps |
|---|---|---|---|---|---|
| BTreeMap + HashMap (reverted to) | **2.562** | 2.528 / 2.586 | **2.734** | **7.71** | 3.16 |
| fixed window + Vec | 2.642 | 2.606 / 2.688 | 2.776 | 8.20 | 3.33 |

The distributions do not overlap, and the bitrate axis was closed earlier (5% of rate
buys nothing), so this is the change and not the scene: ~+0.08 ms decode p50, ~+0.3 ms
e2e. A first attempt at the window was sparse (insert and take scanned all 128 slots);
packing the live entries into `slots[..len]` recovered only ~0.02 ms of it, so the scan
was not the cost.

The lesson, recorded because the same trap is available to anyone: 0.08 ms is ~160k
cycles and no pair of container operations costs that. The likely cost is the *footprint*
- a 128-slot window of ~110-byte entries is ~13 KB inside the decoder struct, while the
BTreeMap touches one or two cache lines when the window is nearly empty, which is the
normal case (the census shows 0-3 outstanding). A fixed window sized to the real maximum
rather than to the historical 128 limit would be the version worth measuring; the
BTreeMap is what ships because it measured best.

`DmabufFrame.planes` stays a `Vec` for the reason below, and it is the last allocation in
the handoff: `sync_fds` is an empty `Vec` (no allocation) and the guard is an inline enum
variant (no box).

## Production baseline

Fixed unless a measurement shows a specific problem:

```
1280x720 @ 60
client: native Cedar, LOW_DELAY=auto, POLL_US=5000, RETRY_US=50, FAST_PLAN=1, ZEROCOPY=1
        zero-copy import failure -> Cedar packed copy -> software only on real breakage
host:   PUNKTFUNK_PIN_CLOCKS=1
```

Open (optional, in order): a long soak for the rare tails (p99 spikes of 18-21 ms exist
in isolated windows) with RSSI/retransmit/quinn-error counters alongside e2e; real games
rather than a desktop scene; and only then an allocation-free Cedar cleanup
(`PtsLedger` BTreeMap -> ring, `held` HashMap -> fixed slots, `DmabufFrame.planes`
Vec -> fixed array). The `PUNKTFUNK_CEDAR_REFUSE_IMPORT` hook is not a hot-path cost -
it reads the environment once through a `OnceLock` - so removing it is tidiness, not
performance.

## Allocation-free cleanup: the ledger and the held map

Two per-frame container changes, no behaviour change, contract unchanged (the existing
`cedar_pts_tests.rs` contract is what they were written against, plus a new test for a
window with holes):

- `PtsLedger` was a `BTreeMap<i64, T>`: one node allocated per submit and freed per take
  - a malloc/free pair per frame in the drain's hot path. It is now a fixed array of 128
  slots (`PTS_SLOTS`, the historical limit): a submit writes a free slot, a take scans
  for the key, a retired slot is reused. Same semantics: nonpositive and duplicate
  submits refused, the 128 limit enforced, arbitrary take order.
- `held` was a `HashMap<u64, *mut VideoPicture>`. The pool is ~14 buffers and the peak
  held is a handful, so hashing bought nothing: it is now a short `Vec` with a linear
  scan (replace on a re-issued token, exactly as the map did).

Not done, and why: `DmabufFrame.planes` is `Vec<DmabufPlane>` in upstream
`pf-client-core`, consumed by the presenter and the other importers. A fixed array does
not express a packed single-plane frame without an accompanying count, so the change is
a cross-crate API change for one small allocation per frame. Measured context: the
handoff's other two candidates are already free - `sync_fds` is an empty `Vec` (no
allocation) and the guard is an inline enum variant (no box). So `planes` is the last
allocation in the path and it is worth ~0.1 us/frame, which is below this device's
measurement floor.

Expected effect of both changes: not mean latency but allocation jitter, so the thing to
watch is the decode tail (p95/p99), not p50.

