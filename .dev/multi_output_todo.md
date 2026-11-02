# Multi-output — completed

Closed on 2026-11-02 after the maintainer reported no remaining problems in
current testing and the final regression/acceptance runs below passed. This file
is retained as the implementation and validation record, not an active backlog.
MoQ retains its experimental designation.

Current behavior is documented in the [output documentation](../docs/output.md)
and [MoQ documentation](../docs/moq.md).

## Completed implementation

- [x] Compose the logo once at the largest HLS rendition's resolution, then
  scale it with the picture for smaller renditions. Pixel comparisons cover
  three unsorted HLS variants, a destination borrowing the smallest rendition,
  and independent outputs with their own composition.
- [x] Reproduce and fix false encoding-group queue overload from live timestamp
  gaps: two tiny entries 1.76 seconds apart formerly disabled the branch.
  Count and memory bounds remain (128 items / 64 MiB); media timestamp span no
  longer represents backlog at this boundary. Real overload logs queue usage.
- [x] Reproduce and fix track-by-track padding after a live reception gap.
  Repeated video and silent audio now enter the output in timestamp order.
  Preserve the original playback error if fallback also fails.
- [x] Reproduce a real 720p live catch-up burst overflowing three HLS encoders
  after a reception pause. Add bounded backpressure (at most 250 ms per incoming
  frame when all active groups are full); keep count/memory budgets and isolate
  blocked branches immediately when another group is ready. A real-codec test
  covers repeated 2.2/4-second pauses, PTS gaps, continuous-PTS bursts and return
  to file playback, verifying decoded video counts and per-track HLS DTS order.
- [x] Allow changing or activating the desktop window without restarting the channel.
- [x] Reproduce RTMP `Input changed` on a mid-stream resolution change and
  rebuild the scaler and letterboxing from actual decoded frame dimensions and
  pixel format. The full RTMP/HLS test fails before this fix and passes after it.
- [x] Release a stale per-track timestamp offset when buffered counterpart
  audio/video confirms the original source clock after a reception gap. Cover
  the previously persistent A/V skew, joint clock resets, and distinct PTS on
  frame-rate-converter gap padding with regression tests.
- [x] Preserve bounded forward video gaps when continuous audio has advanced
  into the gap but has not caught up yet and a second frame confirms the jump.
  A small audio lead alone cannot validate an isolated video timestamp outlier.
  The server's ~1.96-second corrections
  are covered by regression tests, including repeated marked frame/audio
  payloads and decoded RTMP flash/tone alignment through three HLS renditions.
  Irregular markers are paired in order, including checks rejecting whole-period
  shifts and missing markers.
  Confirm recovery under actual OBS bandwidth loss on the affected server.
- [x] Initialize the scaler from the first usable decoded frame instead of
  incomplete stream headers. Reject zero dimensions without panicking and
  prevent audio/queue activity from extending video startup indefinitely.
- [x] Close the historical raw-queue overflow report from 2026-10-27.
  The exact server/source failure was not reproduced, so its original cause
  cannot be claimed as proven. Subsequent timestamp/scaler fixes, real-codec
  live pause/burst regressions, and current maintainer testing have not exposed
  a remaining failure. Count/memory bounds, branch isolation, bounded
  backpressure, and worker-stage/age/PTS diagnostics remain in place. A new
  occurrence should be investigated with those diagnostics and its source dump.
- [x] Decide the delivery/mux worker boundary: retain independently paced workers
  and bounded queues. Worker consolidation is not part of this work package;
  it needs a measured benefit before risking independent retry/isolation.
  No performance improvement from consolidation is claimed or required for closure.
- [x] Replace the local MPEG-TS/TCP importer with direct encoded-frame publishing,
  preserving codec initialization, media timestamps, pacing and isolated retries.

## Final validation, 2026-11-02

- [x] Run the complete non-desktop engine suite on macOS: **437 passed, 6
  intentionally ignored**. Coverage includes real RTMP ingest resolution changes,
  encrypted SRT ingest, reception pauses through three HLS encoders, source-clock
  A/V marker alignment, queue overload/isolation, shared rendition composition,
  independent output changes, and automatic/manual reconnects.
- [x] Add a permanent regression for **two complete retry-exhaustion/manual-resume
  cycles using the same encoded stream**, checking that no automatic connection
  attempts continue after exhaustion. Existing tests cover automatic repeated
  reconnects, the production 15-second / 3-minute policy, and resetting the budget
  after successful delivery.
- [x] Run all **9 MoQ publisher tests**, including shared HLS encoding with video
  lookahead, H.264 B-frame timestamps, HEVC decoder configuration, on-demand audio
  meters, and independent HLS/MoQ toggles without reopening unrelated encoders.
- [x] Add a permanent **36-second / 12-clip** shared HLS/MoQ regression. Alternate
  files with and without VTT, seek into cues, and include invalid zero-duration
  cues alongside valid ones. Verify one continuous MoQ publication, increasing
  media timestamps, A/V tail separation below 100 ms, all **900 HLS video frames**,
  monotonic per-track DTS, retained valid captions, and discarded invalid cues.
  MoQ duration markers are read through the library's container reader rather
  than mistaken for duplicate video frames.
- [x] Run a **72-second / 24-clip** acceptance test with real local FFmpeg RTMP and
  SRT receivers borrowing one HLS encoder. Both receivers exit and restart twice;
  all three sessions per protocol receive media. The unaffected HLS destination
  remains running, retains all **1,800 video frames**, and finishes with less
  than 100 ms A/V tail separation. All HLS/RTMP/SRT recordings have monotonic DTS.
- [x] Retain deterministic coverage for delayed/blocked branches and the shared
  program clock. HLS performs no second realtime wait or write-speed correction;
  the application retains its playlist synchronization/stop-threshold handling.
- [x] Run formatting, Clippy with all features/targets, and whitespace checks.

Reproduction artifacts are in `tmp/multi-output-final/`: `engine.log`, `moq.log`,
`retry.log`, `clippy.log`, plus the network runner (`acceptance.rs`, `network.py`),
receiver logs, recordings, packet reports, and verification results. The temporary
Rust acceptance example is not kept in production sources. Permanent regressions
live beside the delivery and MoQ publisher implementations.

## Scope and continuing operational QA

The runs above are bounded functional/continuity tests, not a new multi-hour
endurance run or an exhaustive browser/device matrix. The maintainer's existing
field tests supplement them. Native MoQ tests verify codec configuration and
published media; they do not claim to exercise browser rendering or WebTransport
through every reverse proxy.

Long-duration browser playback, several-hour HLS timing under filesystem delays,
and memory behaviour on the affected production server remain useful routine QA,
not unfinished implementation tasks in this work package. A high RSS after
reconnects alone does not prove a live-allocation leak: compare allocator used/free
memory and reproduce with a heap profile before attributing it to multi-output.
