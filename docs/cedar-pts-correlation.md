# Cedar exact output PTS / latency attribution

## Vendor token gate

CI-built probe `cad2b17`, device log `phase-test-20261006-152433.log`:
- Input tokens are positive unique `AU sequence * 1000` (diagnostic only).
- Vendor preserved all 1926 output tokens exactly: unmatched 0, outstanding 2
  at close, 1928 AU submitted, 1925 frames returned, 3 empty, 0 decode errors.
- Example: submit AU 16 / token 16000; output AU 13 / token 13000 and AU 14 /
  token 14000. FBM slot IDs are not identities. Delayed output is real.
- This first gate changes no metadata or transport stamps. It proves the
  A523 vendor API is suitable for an exact PTS ledger, not the final latency.

## Exact association

Normal submissions use capture `pts_ns / 1000` as the preferred positive token.
A monotonic checked token clock disambiguates repeats, sub-microsecond collisions,
and regressing/missing capture stamps. The original nanosecond capture stamp is
retained unchanged in the record. A diagnostic probe override remains available.

Before SubmitVideoStreamData, insert a bounded record keyed by the exact token:
source AU id, received timestamp, original host capture timestamp, wire flags,
frame index, planner CedarFacts (IDR/recovery), and colour at submission. On
submit failure remove only that token. At RequestPicture, remove only the exact
VideoPicture.n_pts record; an unknown token fails loudly and the picture still
returns to the vendor on the existing release path. Never choose FIFO position
or the current input AU as a fallback. The ledger is capped at 128; output FIFO
is capped at 32 and retains the previous validated picture policy.

Pixels, matched flags/recovery/colour, and transport stamps move together through
the output FIFO. Pixel-ready time is stamped after copying. Session handoff uses
that pixel-ready time for decode completion, the original receipt for decode age,
and original host PTS for capture-to-ready/capture-to-present telemetry. Time
spent waiting in the output FIFO is thus display-side delay, not falsely counted
as decoder execution. Other decoder rungs retain their original handoff timing.

Unknown output metadata is not relabelled as current-AU metadata. Session refuses
such Cedar output and requests recovery; repeated vendor mismatch goes through
the existing error/demotion ladder. Trusted pairing/identity, stream settings,
presenter/Vulkan, audio, and vendor ABI/configuration are untouched.

## Tests and device gates

Executable Rust tests exercise exact/reordered matching, unknown/duplicate output,
nonpositive/duplicate submission, bounded storage, token uniqueness for repeated
capture timestamps, and retention of original transport flags/metadata. Python
tests exercise both patch transforms and every anchor drift without partial
writes. Patch scripts run before cargo build against the pinned upstream tree.

After CI: independently verify artifact SHA/build metadata, back up and hash-read
back installation, run through Spruce principal ownership, collect actual capture/
received/ready/display distributions, verify output FPS and zero mismatches,
inspect KMS pixels, and prove exact exit/MainUI restoration. Higher truthful HUD
latency is a measurement correction, not a performance regression by itself.
Clock-converted capture latency is not a physical input-to-photon measurement.
DMA-BUF/zero-copy is deliberately deferred until this baseline is verified.
