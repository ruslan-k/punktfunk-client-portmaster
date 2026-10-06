# Cedar output-hold gate candidate

Status: opt-in diagnostic; not enabled for ordinary launches pending live A/B.

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
The first comparison keeps bounded async polling fixed at 5000 us and varies only
this gate; logs retain exact output PTS correlation. A reduction of work alone is
not a latency result: decoded/displayed FPS, lag census, decode/E2E, and physical
DRM frames must also be verified.
