# Live ingest forwarding TODO

Live ingest forwarding and its automated validation are implemented.

## Behavior and configuration

- [x] Configure forwarding destinations under **Settings → Outputs**. Add a media
  source selector: **Playout program** or a specific saved **Live ingest**.
- [x] Show forwarding destinations as links from the corresponding Ingest entry.
  Keep destination configuration in Outputs rather than duplicating it in Ingest.
- [x] Forward the selected ingest independently of whether it currently supplies
  the playout program. Forwarding must also work for standby inputs.
- [x] Support multiple destinations with independent enable/disable, status,
  errors and reconnection, without restarting playout or the ingest listener.
- [x] Distinguish media source selection from the existing program encoding-source
  selection. Present forwarding-specific processing settings for ingest sources.
- [x] Provide **Automatic**, **Copy only** and **Transcode** processing modes.
  Automatic prefers packet copy and transcodes only incompatible tracks. Copy
  only reports incompatibility without silently starting an encoder.
- [x] Report waiting for input/keyframe, copying, transcoding, reconnecting and
  failed states, including why a track needs transcoding.

## Packet path and resource ownership

- [x] Add an optional compressed-packet branch after live demuxing and before
  decoding, scoped to the specific listener and input session. Current decoded
  live-frame events cannot preserve the original encoded packets.
- [x] Publish owned codec parameters, initialization data, stream time bases and
  packet references through a clear engine interface. Keep FFmpeg context
  ownership explicit and avoid borrowed jobs requiring lifetime extension.
- [x] Ensure a stalled or paused program decode path cannot stop ingest forwarding.
  Keep forwarding and program consumption independently bounded.
- [x] Use independent bounded destination queues with nonblocking fan-out and
  reference-counted packet payloads. Slow destinations must not block ingest,
  program playback or other destinations. Resume after overflow at a usable
  keyframe rather than continuing a damaged GOP.
- [x] Preserve audio/video timestamp relationships, rescale time bases and handle
  discontinuities. Tag packets and initialization with session generations so
  reconnects cannot mix old and new media.
- [x] Apply required remuxing and bitstream conversions without decoding when
  possible. Handle codec initialization arriving in-band as well as in extradata.
- [x] Start dynamically enabled video destinations at a usable keyframe with the
  necessary decoder configuration; align audio to the same timeline. Without an
  active ingest, wait without starting unnecessary processing workers.

## Compatibility and transcoding

- [x] Check actual stream parameters against the container, protocol and known
  destination requirements. Codec names alone do not establish compatibility.
  Do not claim to detect remote-server constraints that are not advertised.
- [x] Decide copy/transcode separately for video and audio. Preserve compatible
  tracks even when another track requires encoding.
- [x] Cover SRT/MPEG-TS H.264 + AAC → RTMP/FLV as a primary packet-copy case,
  including packet format conversion, codec headers and timestamp normalization.
- [x] Treat resolution changes and requested processing as explicit reasons to
  transcode the affected track. Do not apply program logos, overlays or audio
  processing to direct forwarding by default.
- [x] Share equivalent transcode profiles across destinations. Reuse compatible
  decoded input frames where practical without coupling destination lifetimes or
  accidentally reusing frames modified for the playout program.
- [x] Reevaluate compatibility on a new ingest session or codec change and rebuild
  affected deliveries/transcoders. Network failures must never trigger transcoding.
- [x] Reuse existing destination delivery/retry behavior where applicable: retry
  every 15 seconds for up to three minutes, reset after successful delivery, and
  allow manual retry after exhaustion. Ingest sessions and keyframe gating need
  explicit integration rather than assuming program-output behavior is sufficient.

## Dynamic lifecycle and idle cost

- [x] Apply forwarding configuration through runtime updates. Stop only affected
  destinations; keep other forwards and playout running.
- [x] Create no forwarding threads, encoders, packet references or media buffers
  when no forwarding destination is enabled. Retain the existing ingest cost;
  do not describe an already running listener/decoder as resource-free.
- [x] Stop and release unused transcoders when their last consumer disappears.
  Do not tear down resources still required by program playback or other forwards.
- [x] Make queue waits and blocking I/O cancellable; close and join workers on
  disable, session replacement and shutdown. Bound shutdown and reconnect work.
- [x] Add configuration persistence, validation and authorized runtime API updates,
  including clear handling when a referenced ingest is removed.

## Implementation sequence

- [x] First implement packet/session publication and multiple dynamic **Copy only**
  destinations. Validate isolation and lifecycle before adding automatic fallback.
- [x] Add per-track automatic compatibility decisions and transcoding.
- [x] Add shared transcode groups and frontend status/details.
- [x] Document supported combinations, behavior on input loss, and experimental
  limitations before release.

## Validation

- [x] Verify SRT → RTMP packet copy with real endpoints, decoding the output and
  checking that no additional decoder or encoder is started for forwarding.
- [x] Verify audio-only transcoding, video-only transcoding and full transcoding.
- [x] Test simultaneous targets, shared transcoding and forwarding a standby
  ingest while the program plays a playlist or another live input.
- [x] Repeatedly enable/disable forwarding during playback without listener or
  channel restarts. Verify disabled forwarding adds no ongoing processing work.
- [x] Test input loss/reconnect, timestamp resets, changed codecs/resolution,
  missing tracks, late initialization and long GOPs.
- [x] Test destination failure, slow writes, queue overflow, repeated reconnects
  and manual recovery while other destinations and program playback continue.
- [x] Run sustained native Linux tests monitoring RSS/PSS, CPU, thread counts,
  queue bounds, audio/video synchronization and shutdown resource release.

- The playlist integration test switches two standby forwarding destinations on
  and off twice. The publisher session stays the same, the playlist keeps playing
  and the forwarding threads exit after each disable. The transcode test checks
  that matching destinations use one encoding group.
- A real SRT publisher reconnects with a different video codec and resolution,
  then with audio only. A generated four-second GOP checks late in-band headers,
  backwards timestamps and codec changes within a session.
- Pausing an RTMP receiver on Linux blocks an active write. The queue discards
  stale packets at its bound, and disabling the destination cancels the write
  and releases its queue. Separate tests exercise both byte and packet limits.
- Regression tests cover a closing transcode group, recovery after a damaged
  AAC packet, and muxer delivery while an announced track is silent. The live
  watchdog keeps the video startup deadline until a usable frame arrives.
- Incompatible copy routes leave program backpressure in effect and return to
  waiting for input when the publisher disconnects. The delivery retry budget
  resets once AVIO has written media beyond the output header.
- A four-minute 720p Linux loopback run with an active playlist waited through
  automatic RTMP retries and the full three-minute retry window, then recovered
  the failed destination manually. The other destination and playlist both
  continued. Packet checks found a maximum gap of 40 ms in all three captures,
  with maximum observed A/V DTS separation of 38 ms in both forwards and 107 ms
  in the playlist. The test also checked forwarding thread cleanup.
- During steady forwarding and retries, the 720p debug test process used
  240–283 MiB RSS, 205–247 MiB PSS, 47–54 threads and about 27% of one CPU core.
  RSS peaked at about 289 MiB in the entire run. These figures describe local
  loopback over four minutes.

Run all forward tests, including the FFmpeg loopback tests:

```sh
cargo test -p ff-engine --lib output::forwarding -- --include-ignored --test-threads=1
```

Run the full retry window and manual reconnect at 720p:

```sh
FFPLAYOUT_FORWARD_TEST_SECONDS=240 FFPLAYOUT_FORWARD_TEST_SIZE=1280x720 \
  cargo test -p ff-engine --lib forwarding_during_playlist \
  -- --ignored --test-threads=1
```
