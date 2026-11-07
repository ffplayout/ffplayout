# HLS packager backlog

Proposed native orchestration of HLS segments, playlists, WebVTT and timed
advertising events. This is an exploration, not an implementation commitment;
see [Development Notes](README.md). Keep the existing FFmpeg HLS path available
until a replacement passes compatibility and long-running tests.

## Scope and architecture

- [ ] Write a short architecture decision before implementation: ffplayout owns
  segment boundaries, timeline mapping, playlist publication and retention;
  FFmpeg initially retains MPEG-TS/fMP4 container muxing and existing encoding.
  A playlist serializer alone is not a complete live packager.
- [ ] Separate timeline/segment state, container writing, manifest serialization,
  subtitle segmentation and timed events behind small interfaces. Evaluate a
  workspace crate for reusable HLS logic; keep engine integration and FFmpeg
  adapters separate from application configuration, database and HTTP handlers.
- [ ] Reuse encoded packets and existing encoding groups. Preserve independent,
  bounded output delivery, recording/desktop behavior and the current largest-
  rendition HLS composition/downscaling. Do not introduce duplicate encoders.
- [ ] Audit packet interfaces for owned codec initialization, stream time bases,
  PTS/DTS/duration, keyframe information and output generations. The current
  `PacketSink`/`EncodedFrame` boundary must not be assumed sufficient without
  checking these requirements.
- [ ] First prove MPEG-TS HLS with multiple variants and WebVTT. Treat fMP4/CMAF,
  LL-HLS, encryption/DRM and ad-server integration as separate follow-ups; retain
  the existing backend for configurations the new path does not support.

## Crate evaluation

- [ ] Compare [m3u8-rs](https://docs.rs/m3u8-rs/latest/m3u8_rs/) and
  [hls_m3u8](https://docs.rs/hls_m3u8/latest/hls_m3u8/) with small prototypes for
  master/media playlists, alternate subtitles, PROGRAM-DATE-TIME, discontinuity
  sequences, DATERANGE and custom attributes needed by interstitials.
- [ ] Consider [broadcast-hls](https://docs.rs/broadcast-hls/latest/broadcast_hls/)
  and [hls-runtime](https://docs.rs/hls-runtime/latest/hls_runtime/) as additional
  candidates/reference architectures. Verify typed versus opaque tag support,
  dependencies and integration cost rather than assuming advertised features
  are sufficient.
- [ ] Evaluate [scte35](https://docs.rs/scte35/latest/scte35/) for message
  generation/parsing, CRC validation and known wire-format fixtures. Keep this
  optional and independent of basic HLS output.
- [ ] Review maintenance, license compatibility, supported Rust versions,
  malformed-input handling, precision and round-trip fidelity before choosing
  dependencies. Pure Rust container crates such as
  [mpeg2ts](https://docs.rs/mpeg2ts/latest/mpeg2ts/) are a later option, not a
  prerequisite for replacing FFmpeg's HLS orchestration.

## Shared media timeline and segment coordination

- [ ] Define a common presentation timeline for video, audio, WebVTT and events.
  Keep media time distinct from sequence numbers, filenames and wall-clock time.
  Use explicit integer time bases and checked conversions.
- [ ] Coordinate independently decodable segment boundaries across renditions.
  Align keyframes and account for encoder delay/B-frames and audio frame
  boundaries; never cut every stream independently on elapsed wall-clock time.
- [ ] Anchor PROGRAM-DATE-TIME consistently across all renditions and captions.
  Define behavior for wall-clock corrections without silently shifting media
  timestamps or creating conflicting absolute-time mappings.
- [ ] Propagate timestamp resets, codec changes and restarts through consistent
  EXT-X-DISCONTINUITY and EXT-X-DISCONTINUITY-SEQUENCE values in every affected
  media playlist, including subtitle playlists.

## WebVTT synchronization

- [ ] Measure the actual relationship between emitted video PES presentation
  timestamps and ffplayout subtitle timestamps. Account for FFmpeg muxer offsets;
  do not assume that converting milliseconds to 90 kHz alone establishes it.
- [ ] Generate X-TIMESTAMP-MAP for each WebVTT segment using that relationship.
  Handle the 33-bit MPEG-TS timestamp wrap and recompute the mapping for a new
  discontinuity generation. Existing resumed segments keep their own mapping.
- [ ] Write the header before publishing the segment. Avoid a polling repair job
  or modifications to subtitle segments already available to clients. Defer the
  previously discussed standalone FFmpeg VTT-header adapter until the packager
  prototype establishes whether it is still needed.
- [ ] Include every cue intersecting a segment's presentation interval, including
  cues spanning boundaries. Define empty-segment behavior so long subtitle gaps
  do not stall A/V, and retain tolerant handling of malformed source cues.

## Publication, resume and retention

- [ ] Publish completed media before atomically publishing playlists that refer
  to it. Generate accurate durations/target durations and stable variant/subtitle
  associations; expose no partial manifests or missing referenced resources.
- [ ] Preserve usable playlists and referenced segments across normal restarts.
  Continue segment numbering without overwriting cached URLs with different
  bytes, and signal a new timestamp generation where required.
- [ ] Specify recovery for interrupted writes, inconsistent variants, missing
  segments and incompatible codec initialization. Preserve the current stale-
  resume policy (currently 60 seconds) unless an explicit design decision changes
  it; do not introduce unconditional full cleanup on restart.
- [ ] Retain enough unreferenced media for clients already fetching it. Delete
  only resources owned by this output after its retention period. Preserve
  unrelated outputs and exclusive ownership until the old writer has stopped.
- [ ] Keep memory/disk use bounded and report publication, ownership and recovery
  failures per destination. A stalled HLS destination must not stall healthy
  outputs or cause an unbounded restart loop.

## Advertising and other timed events

- [ ] Model event ID, actual media start/end, planned duration, event type and
  optional payload independently of HLS tag serialization. Define how existing
  playlist `ad` items group into breaks; do not infer blocks from filenames.
- [ ] Specify event behavior for trimmed/skipped clips, playlist edits, fallback,
  live takeover and return. Emit markers for what is actually broadcast, with
  stable IDs across playlist refreshes and explicitly defined restart recovery.
- [ ] Support EXT-X-DATERANGE as the common HLS event representation. Map SCTE-35
  splice commands into SCTE35-OUT/IN/CMD when that integration is enabled, using
  correct absolute dates and actual segment/splice boundaries.
- [ ] Evaluate HLS Interstitials separately: asset URI/list, duration and resume
  behavior, player support and behavior for unsupported clients. Merely emitting
  an ad marker does not implement ad selection, insertion, tracking or SSAI.
- [ ] Add vendor-specific CUE-OUT/CUE-IN tags only for an identified consumer;
  keep them adapters rather than the internal event model. Design configuration,
  playlist/API controls and any necessary persistence only after semantics are
  established. Keep the XMLTV advertising policy in [epg_todo.md](epg_todo.md)
  consistent without making EPG export depend on this packager.

## Verification and rollout

- [ ] Differentially compare generated media/manifests with the current backend:
  codec declarations, rendition alignment, seekability, subtitles and A/V sync.
- [ ] Test later player entry, variant switches, subtitle enable/disable, empty
  cues, cross-segment cues, clip transitions, live takeover and return.
- [ ] Test restart/resume, abrupt termination, concurrent destination attempts,
  slow/full storage, missing files and clock corrections with scoped clocks.
- [ ] Simulate timestamps around 26.5 and 53 hours to cover multiple MPEG-TS wraps;
  run a separate 24/7 soak test for resource stability and recovery.
- [ ] Verify actual playback with our Video.js/hls.js frontend, Safari/native HLS
  and another independent player. Test advertising marker consumers and
  interstitial-capable clients separately; parser round trips are insufficient.
- [ ] Keep rollout opt-in until parity and failure isolation are demonstrated.
  Compare CPU, memory, I/O and latency; document unsupported configurations and
  a deliberate rollback path without simultaneous writers to one HLS target.

**First milestone:** a prototype that consumes existing encoded renditions,
produces aligned MPEG-TS segments and atomically published playlists/WebVTT with
verified timestamp mapping, and resumes without unnecessarily deleting media.
Advertising serialization follows after that shared timeline is validated.

## Protocol references

- [RFC 8216: WebVTT and X-TIMESTAMP-MAP](https://www.rfc-editor.org/rfc/rfc8216.html#section-3.5)
- [RFC 8216: PROGRAM-DATE-TIME](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.6)
- [RFC 8216: SCTE-35 in DATERANGE](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.7.1)
- [Apple: Getting Started with HLS Interstitials](https://developer.apple.com/streaming/GettingStartedWithHLSInterstitials.pdf)
