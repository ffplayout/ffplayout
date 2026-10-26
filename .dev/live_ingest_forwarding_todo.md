# Live ingest forwarding TODO

Proposed follow-up after 2.7.0. This is a design outline; implementation and
validation are still pending.

## Behavior and configuration

- [ ] Configure forwarding destinations under **Settings → Outputs**. Add a media
  source selector: **Playout program** or a specific saved **Live ingest**.
- [ ] Show forwarding destinations as links from the corresponding Ingest entry.
  Keep destination configuration in Outputs rather than duplicating it in Ingest.
- [ ] Forward the selected ingest independently of whether it currently supplies
  the playout program. Forwarding must also work for standby inputs.
- [ ] Support multiple destinations with independent enable/disable, status,
  errors and reconnection, without restarting playout or the ingest listener.
- [ ] Distinguish media source selection from the existing program encoding-source
  selection. Present forwarding-specific processing settings for ingest sources.
- [ ] Provide **Automatic**, **Copy only** and **Transcode** processing modes.
  Automatic prefers packet copy and transcodes only incompatible tracks. Copy
  only reports incompatibility without silently starting an encoder.
- [ ] Report waiting for input/keyframe, copying, transcoding, reconnecting and
  failed states, including why a track needs transcoding.

## Packet path and resource ownership

- [ ] Add an optional compressed-packet branch after live demuxing and before
  decoding, scoped to the specific listener and input session. Current decoded
  live-frame events cannot preserve the original encoded packets.
- [ ] Publish owned codec parameters, initialization data, stream time bases and
  packet references through a clear engine interface. Keep FFmpeg context
  ownership explicit and avoid borrowed jobs requiring lifetime extension.
- [ ] Ensure a stalled or paused program decode path cannot stop ingest forwarding.
  Keep forwarding and program consumption independently bounded.
- [ ] Use independent bounded destination queues with nonblocking fan-out and
  reference-counted packet payloads. Slow destinations must not block ingest,
  program playback or other destinations. Resume after overflow at a usable
  keyframe rather than continuing a damaged GOP.
- [ ] Preserve audio/video timestamp relationships, rescale time bases and handle
  discontinuities. Tag packets and initialization with session generations so
  reconnects cannot mix old and new media.
- [ ] Apply required remuxing and bitstream conversions without decoding when
  possible. Handle codec initialization arriving in-band as well as in extradata.
- [ ] Start dynamically enabled video destinations at a usable keyframe with the
  necessary decoder configuration; align audio to the same timeline. Without an
  active ingest, wait without starting unnecessary processing workers.

## Compatibility and transcoding

- [ ] Check actual stream parameters against the container, protocol and known
  destination requirements. Codec names alone do not establish compatibility.
  Do not claim to detect remote-server constraints that are not advertised.
- [ ] Decide copy/transcode separately for video and audio. Preserve compatible
  tracks even when another track requires encoding.
- [ ] Cover SRT/MPEG-TS H.264 + AAC → RTMP/FLV as a primary packet-copy case,
  including packet format conversion, codec headers and timestamp normalization.
- [ ] Treat resolution changes and requested processing as explicit reasons to
  transcode the affected track. Do not apply program logos, overlays or audio
  processing to direct forwarding by default.
- [ ] Share equivalent transcode profiles across destinations. Reuse compatible
  decoded input frames where practical without coupling destination lifetimes or
  accidentally reusing frames modified for the playout program.
- [ ] Reevaluate compatibility on a new ingest session or codec change and rebuild
  affected deliveries/transcoders. Network failures must never trigger transcoding.
- [ ] Reuse existing destination delivery/retry behavior where applicable: retry
  every 15 seconds for up to three minutes, reset after successful delivery, and
  allow manual retry after exhaustion. Ingest sessions and keyframe gating need
  explicit integration rather than assuming program-output behavior is sufficient.

## Dynamic lifecycle and idle cost

- [ ] Apply forwarding configuration through runtime updates. Stop only affected
  destinations; keep other forwards and playout running.
- [ ] Create no forwarding threads, encoders, packet references or media buffers
  when no forwarding destination is enabled. Retain the existing ingest cost;
  do not describe an already running listener/decoder as resource-free.
- [ ] Stop and release unused transcoders when their last consumer disappears.
  Do not tear down resources still required by program playback or other forwards.
- [ ] Make queue waits and blocking I/O cancellable; close and join workers on
  disable, session replacement and shutdown. Bound shutdown and reconnect work.
- [ ] Add configuration persistence, validation and authorized runtime API updates,
  including clear handling when a referenced ingest is removed.

## Implementation sequence

- [ ] First implement packet/session publication and multiple dynamic **Copy only**
  destinations. Validate isolation and lifecycle before adding automatic fallback.
- [ ] Add per-track automatic compatibility decisions and transcoding.
- [ ] Add shared transcode groups and frontend status/details.
- [ ] Document supported combinations, behavior on input loss, and experimental
  limitations before release.

## Validation

- [ ] Verify SRT → RTMP packet copy with real endpoints, decoding the output and
  checking that no additional decoder or encoder is started for forwarding.
- [ ] Verify audio-only transcoding, video-only transcoding and full transcoding.
- [ ] Test simultaneous targets, shared transcoding and forwarding a standby
  ingest while the program plays a playlist or another live input.
- [ ] Repeatedly enable/disable forwarding during playback without listener or
  channel restarts. Verify disabled forwarding adds no ongoing processing work.
- [ ] Test input loss/reconnect, timestamp resets, changed codecs/resolution,
  missing tracks, late initialization and long GOPs.
- [ ] Test destination failure, slow writes, queue overflow, repeated reconnects
  and manual recovery while other destinations and program playback continue.
- [ ] Run sustained native Linux tests monitoring RSS/PSS, CPU, thread counts,
  queue bounds, audio/video synchronization and shutdown resource release.
