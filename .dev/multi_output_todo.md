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
- [x] Allow changing or activating the desktop window without restarting the channel.
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
