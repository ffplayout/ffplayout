# Multi-output TODO

Current behavior is documented in the [output documentation](../docs/output.md)
and [MoQ documentation](../docs/moq.md).

## Future improvements

- [x] Allow changing or activating the desktop window without restarting the channel.
- [ ] Profile the remaining paced network delivery/mux boundary before combining
  those workers; preserve independent reconnects, pacing, and bounded buffering.
- [x] Replace the local MPEG-TS/TCP importer with direct encoded-frame publishing,
  preserving codec initialization, media timestamps, pacing and isolated retries.

## Validation

- [ ] Run a sustained multi-output test covering clip transitions, live takeover,
  slow or disconnected destinations, and long-term audio/video timing.
- [ ] Verify sustained browser audio/video playback with the multi-output runtime.
- [ ] Test sustained recovery with external RTMP/SRT servers, including a second
  outage after a successful reconnect and manual reconnect after retry exhaustion.
- [ ] Verify HLS timing over several hours with shared MoQ encoding, including
  temporary write delays and playlist stop-threshold handling.
- [ ] Verify HLS/WebVTT clip transitions and seeking with alternating files with
  and without subtitles during a sustained run.
