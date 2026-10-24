# Known issues

## MPEG-TS input can fall back after seeking

**Status:** Reproduced in an isolated engine test on 2026-10-23; cause unresolved.

A local MPEG-TS file created by stream-copy remuxing
`tests_assets/storage/media_mix/with_audio.mp4` (H.264/AAC) failed to play a
one-second excerpt starting at two seconds through `AsyncPlayout`.
The decoder reported `co located POCs unavailable` and
`mmco: unref short failure`. The engine then returned:

```text
Fallback: input pipeline stalled waiting for selected audio/video tracks
```

Scope is currently limited to this fixture and seek position. This does not
establish that all TS inputs, other seek positions, or playback without seeking
fail. It concerns MPEG-TS **input**, not HLS segment output.

- [ ] Reproduce with additional TS files and seek positions, including zero
  and positions around keyframes; compare playback without seeking.
- [ ] Investigate keyframe selection, decoder recovery, input timestamp origin,
  and the stalled-track fallback. These are investigation areas, not confirmed
  causes.
- [ ] Add a regression test when the cause is understood and fixed.
