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
