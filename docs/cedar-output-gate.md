# Cedar output-hold gate candidate

Status: live one-axis A/B passed. Runtime defaults now request guarded `auto` and
bounded 5000 us polling; explicit 0/0 restores the original vendor path.

Read-only analysis of the actual TSPS libraries found a previously untranscribed
VConfig word. No firmware, library instruction, or reference-picture data is patched.

- libawh264.so SHA-256: 052e33068070d08e0d324fc181b0381f3ac071f02f014c65605cc51d1a26f373.
- H264DecoderInit at 0x17ab4 copies 216 bytes into context+0x68.
- H264SortDisplayFrameOrder at 0x21928 loads context+0x128, masks bit 0,
  and gates the branch to 0x22208 (return without display publication).
- Therefore this gate lives at VConfig+0xc0 = byte 192. Its SDK field name is
  unknown; `common_config_flags_192` is an observational local name, not a header claim.
- Candidate `PUNKTFUNK_CEDAR_LOW_DELAY=1` sets only that word to 1.
- Two exact instruction-window checks refuse a different vendor implementation.
- The H.264 planner must confirm progressive POC type 2 and no B slices.
  Later AU drift or planning failure is rejected before feeding it to the hardware.
- Structure size stays 216 bytes and all prior field offsets are preserved.

Controls keep the default word zero. All existing vendor knobs remain unchanged.
The comparison keeps bounded async polling fixed at 5000 us and varies only
this gate; logs retain exact output PTS correlation. The default `auto` enables
the gate only when both vendor and stream checks pass; otherwise the prior
configuration is retained. Strict 1 refuses an unsupported vendor/stream.

## Live A/B: CI build 4c48a1f

Quiet 1280x720 H.264 desktop scene, actual reception about 19.8 FPS.
Only the VConfig+192 word varied; poll budget remained 5000 us.

Latencies below are sample-weighted means, not an inferred global P50.

### gate-control
- decode: 105.822 ms; E2E: 114.356 ms.
- received/decoded/presented medians: [19.822, 19.822, 19.822] FPS.
- AU-to-output lag histogram [0,1,2,3,4+]: [0, 1, 1392, 0, 0].
- Close: `aus=1395 frames=1393 empties=2 errors=0 pts_matches=1393 pts_unmatched=0 pts_outstanding=2 output_lag_frames=[0, 1, 1392, 0, 0]`.
- Log SHA-256: `c935e5a4dfe35778dec2d5ae4011d2cce54abe40cbcfccd6f9da1b8ceb6bec90`.
- lost=0; skipped=0; session_exit=0.

### gate-one
- decode: 4.976 ms; E2E: 13.628 ms.
- received/decoded/presented medians: [19.822, 19.841, 19.841] FPS.
- AU-to-output lag histogram [0,1,2,3,4+]: [1396, 0, 0, 0, 0].
- Close: `aus=1396 frames=1396 empties=0 errors=0 pts_matches=1396 pts_unmatched=0 pts_outstanding=0 output_lag_frames=[1396, 0, 0, 0, 0]`.
- Log SHA-256: `6a6f8371085ddc6a8ea48809fedcd4af694ac65b1beb3bc315c910270989a355`.
- lost=0; skipped=0; session_exit=0.

Both real KMS frames are clean and show native-cedar. MainUI returned after each
bounded run. No system library was modified. Paired state was preserved.

This is a desktop-stream decode/display result, not a verified gameplay or
input-to-photon benchmark.

## Final runtime-default verification: 9badbe4

A 180-second quiet run used the shipped runtime defaults, no tuning/profile/PTS
probe overrides. The planner reported progressive POC type 2; auto verified both
instruction windows and enabled the output gate. Bounded polling was 5000 us.

- Native Cedar, actual received/decoded/presented median 19.841 FPS.
- Sample-weighted mean decode 5.024 ms; E2E 13.495 ms.
- Median of per-window P50 decode 5.067 ms (not global frame P50).
- 3479 AU / 3479 outputs; all 3479 exact PTS matches, 0 unmatched/outstanding.
- AU-to-output lag [0,1,2,3,4+]: [3479, 0, 0, 0, 0].
- Errors/lost/skipped: 0. session_exit=0.
- Final client SHA-256: `f5245883eac67bde2b894a6be8016732fd1aa2be10f38677e39ec4cd5ee2109a`.
- Runtime SHA-256: `1576346afb0cab6465247706e6d886f09406ff785b64f5b91068305ba5e4b76f`.
- Log SHA-256: `35c438f4f4b8927ac6ad773aa653a51b482929390416dfef38620db862d21314`.
- The actual stream frame was clean and showed native-cedar / decode 5.0 ms.
- A separate real KMS capture verified return to Spruce/PyUI; no Punktfunk
  process, temporary test launcher, or queued principal command remained.
- Device and pulled vendor library hashes matched; no system library was changed.
