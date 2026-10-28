# Multi-output TODO

Current behavior is documented in the [output documentation](../docs/output.md)
and [MoQ documentation](../docs/moq.md).

## Future improvements

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
- [ ] Diagnose the remaining raw-queue overflow reported at 20:05:33 and
  20:06:27 on 2026-10-27 despite bounded backpressure (48 720p frames / 64 MiB).
  Local end-to-end RTMP runs with libfdk_aac, reception pauses, Full-Range
  video, a sender PTS jump, and a separate ready encoding group have not
  reproduced that remaining failure. The supplied server configuration also
  passes with one HLS encoding group, shared native 480p delivery, shared 720p
  FLV, VTT, logo, and program loudness; external MoQ/RTMP transport and the exact
  source stream are not reproduced. Enable timing diagnostics on the server to
  capture the encoding worker stage/age, rendition and last A/V PTS on overflow.
- [ ] Profile the remaining paced network delivery/mux boundary before combining
  those workers; preserve independent reconnects, pacing, and bounded buffering.
- [x] Replace the local MPEG-TS/TCP importer with direct encoded-frame publishing,
  preserving codec initialization, media timestamps, pacing and isolated retries.

## Validation

- [ ] Verify the 2026-10-27 RTMP failure scenario on the affected server with
  repeated reception interruptions and reconnects. Local deterministic
  regressions reproduce the two corrected failure mechanisms, but do not prove
  the server had no additional bottleneck. Check for duplicate logos in its HLS
  configuration. Track RSS and glibc used/free memory: the supplied log ended
  near 1.17 GB RSS, with about 288 MB allocated and 882 MB free inside arenas;
  this alone does not establish a live-allocation leak.

- [ ] Run a sustained multi-output test covering clip transitions, live takeover,
  slow or disconnected destinations, and long-term audio/video timing.
- [ ] Verify sustained browser audio/video playback with the multi-output runtime.
- [ ] Test sustained recovery with external RTMP/SRT servers, including a second
  outage after a successful reconnect and manual reconnect after retry exhaustion.
- [ ] Verify HLS timing over several hours with shared MoQ encoding, including
  temporary write delays and playlist stop-threshold handling.
- [ ] Verify HLS/WebVTT clip transitions and seeking with alternating files with
  and without subtitles during a sustained run.
