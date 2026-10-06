# Cedar output-hold gate candidate

Status: live one-axis A/B passed. Runtime defaults now request guarded `auto` and
bounded 5000 us polling; explicit 0/0 restores the original vendor path.

Read-only analysis of the actual TSPS libraries found a previously untranscribed
VConfig word. No firmware, library instruction, or reference-picture data is patched.

- libawh264.so SHA-256: 318697525414467680c2c6355b23b7c9b43abfba2f369499d0849c07bd7c94a8.
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
input-to-photon benchmark. Final ordinary-launch verification is tracked separately.
