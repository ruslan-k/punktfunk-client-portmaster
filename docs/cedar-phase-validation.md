# Cedar phase census and FIFO validation

## Scope

TSPS, 1280x720 H.264, 30 Hz requested, vendor DEBUG disabled. No presenter,
Vulkan, decoder ABI, audio, holding-count, or server configuration changes.
Source commits: instrumentation `b0c7009`, opt-in FIFO candidate `40d30f6`.
Both distributed binaries built by GitHub Actions.

## Counters (bounded desktop-stream runs)

Control log `phase-test-20261006-144710.log`, 70 stats windows:
- Received 1818; decoded 909; presented 909.
- Complete census batches (1800 AU): copied 1797 pictures, internally replaced
  897 pictures, returned 900. Drain histogram [0, 1, 2, >=3 pictures] =
  [900, 3, 897, 0]. At steady state: 120 input AU, 120 pictures copied,
  60 pictures overwritten, 60 returned. This localizes the 2:1 loss before
  the session/presenter, to `newest = Some(frame)` in Cedar drain.
- Vendor FRAME_DECODED mean 2.426 ms; CPU picture copy mean 3.601 ms.
- NO_BITSTREAM calls take tens of microseconds, not a 10 ms wait.

FIFO candidate log `phase-test-20261006-145714.log`, 70 stats windows:
- Received 1389; decoded 1386; presented 1386.
- Complete census batches (1320 AU): copied 1317 pictures, internally replaced
  0, returned 1317. Drain histogram = [659, 5, 656, 0]. Queue remains bounded
  at 1-2 pictures during the run, rather than accumulating a growing backlog.
- Vendor FRAME_DECODED mean 2.485 ms; CPU picture copy mean 3.579 ms.
- Actual host reception was mostly 20 FPS in this window, so these two runs
  do **not** establish matched-scene 30 FPS performance. They do establish
  elimination of Cedar's internal 50% discard: all but startup-tail pictures
  reach presentation.

## Latency and acceptance boundaries

Median across window decode P50 values: control 13.067 ms, FIFO 13.071 ms.
These HUD figures still associate output with the *current input AU*; vendor
PTS was submitted as -1. They cannot establish correct picture age or a real
latency win. FIFO restores picture count, not proof of lower end-to-end delay.
The proper next boundary is explicit submitted-PTS to output-picture matching,
including transport stamps and per-picture recovery facts, before tuning
buffering or claiming a lower HUD number as a gain.

Physical KMS captures show correctly coloured Steam UI in both runs and the
Spruce menu after exit. Exact client processes exit and MainUI resumes.
ALSA playback setup succeeds in the logs; these autonomous runs do not prove
subjective audio quality or physical gameplay/input.

## Runtime controls

`PUNKTFUNK_CEDAR_FIFO=0` restores the original newest-wins control for A/B.
Default uses FIFO with a 32-picture fail-closed backlog guard.
`PUNKTFUNK_CEDAR_PROFILE=1` enables bounded phase counters and sampled queue/
picture metadata. Leave profiling off for ordinary use. Counter stage indices:
0 planner, 1 feed, 2 whole decode call, 3 RequestPicture, 4 copy,
5 ReturnPicture. Stage 2 overlaps its children; stage 1 includes SBM-full
retirement when it occurs. Vendor-code index 7 aggregates out-of-range codes.
`au`/`frames` are cumulative; all duration/count arrays are window-local.

Current output colour/recovery metadata and transport stamps still follow
current-AU integration; this phase does not claim B-frame/reordered/recovery
correctness. Further work must match actual output PTS, not FBM slot IDs.
