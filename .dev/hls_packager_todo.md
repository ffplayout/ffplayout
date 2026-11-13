# HLS packager backlog

Native orchestration of HLS segments, playlists and WebVTT, with timed
advertising events planned separately. The opt-in implementation lives in `crates/hls`;
see [Development Notes](README.md). Keep the existing FFmpeg HLS path available
until a replacement passes compatibility and long-running tests.

Status reconciled with `main` and the open changes on 2026-11-13. Checked items
describe implemented behavior with code/test evidence, not production qualification.
The current core suite has 23 tests; the documented Linux comparison ran the
earlier 17-test suite. Engine/browser results recorded in the comparison document
are historical and were not rerun as part of this reconciliation.

## Initial implementation status

- [x] Add the FFmpeg-independent `ff-hls` crate and the engine's MPEG-TS adapter.
- [x] Add per-output FFmpeg/native selection, defaulting to FFmpeg, using existing
  persisted muxer options. A change recreates only that output.
- [x] Prototype aligned H.264/AAC variants, WebVTT with X-TIMESTAMP-MAP, atomic
  individual playlist publication and bounded packet storage.
- [x] Test real encoders, resume, fresh starts on backend switches and sharing
  existing encoders with the FFmpeg backend.
- [x] Keep nominal cuts relative to session origin; test late/missing keyframes,
  mismatched rendition boundaries, packet preservation and crossing captions.
- [x] Repeat the 30-run FFmpeg/native comparison and run the 17 core tests on
  Linux `/dev/shm`; complete short paced runs and a Safari native-HLS smoke test.
  Full player qualification and a long real-time soak remain below.
- [x] Protect short live windows with a three-target-duration minimum, preserve
  TARGETDURATION across resume and reject incompatible increases without deleting
  retained media. Allow `_vtt` rendition suffixes when subtitles are disabled.
- [x] Write consistent optional PROGRAM-DATE-TIME across video and subtitle
  playlists; test enabled/disabled output and preservation of retained dates on resume.
- [x] Default to `hls_flags=program_date_time` when explicitly selecting Native in
  the web interface without existing `hls_flags`. Keep existing native settings
  unchanged and allow removal of the option. API clients set it explicitly.
- [x] Document FFmpeg's missing subtitle PROGRAM-DATE-TIME and the reproduced
  hls.js 1.7.3/1.7.4 live-subtitle alignment issue. Distinguish this investigation
  from qualification of a real native HLS stream.
- [ ] Qualify production playback and long-running behavior before promotion.

See [architecture decision](../docs/development/hls-packager.md). The remaining
unchecked items below cover broader qualification and capabilities beyond the
initial local MPEG-TS prototype.

## Scope and architecture

- [x] Write a short architecture decision before implementation: ffplayout owns
  segment boundaries, timeline mapping, playlist publication and retention;
  FFmpeg initially retains MPEG-TS/fMP4 container muxing and existing encoding.
  A playlist serializer alone is not a complete live packager.
- [x] Put reusable HLS logic in a separate workspace crate, keeping engine/FFmpeg
  adapters separate from application configuration, database and HTTP handlers.
  Packet/cue models, container writing and manifest serialization have separate modules.
- [ ] Extract subtitle segmentation from the shared segmenter and define a
  dedicated timed-event interface when extending beyond the initial prototype.
- [x] Reuse encoded packets and existing encoding groups. Preserve independent,
  bounded output delivery, recording/desktop behavior and the current largest-
  rendition HLS composition/downscaling. Do not introduce duplicate encoders.
- [x] Audit packet interfaces for owned codec initialization, stream time bases,
  PTS/DTS/duration, keyframe information and output generations. The current
  `PacketSink`/`EncodedFrame` boundary must not be assumed sufficient without
  checking these requirements.
- [x] First prove MPEG-TS HLS with multiple variants and WebVTT. Treat fMP4/CMAF,
  LL-HLS, encryption/DRM and ad-server integration as separate follow-ups; retain
  the existing backend for configurations the new path does not support.

## Crate evaluation

The initial prototype uses its own limited playlist serializer. The following
evaluations are deferred extension work, not prerequisites for that prototype.

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

- [x] Define a common integer-microsecond packet timeline and millisecond cue
  timeline for video, audio and WebVTT. Keep media time distinct from sequence
  numbers, filenames and wall-clock time; preserve container PTS with `mpegts_copyts`.
- [ ] Extend that timeline to events and audit remaining arithmetic/conversions
  for overflow at extreme timestamps and long runtimes.
- [x] Coordinate independently decodable segment boundaries across renditions.
  Align keyframes and account for encoder delay/B-frames and audio frame
  boundaries; never cut every stream independently on elapsed wall-clock time.
- [x] Anchor optional PROGRAM-DATE-TIME once per session and derive matching dates
  from media time for all renditions and captions; preserve retained segment dates.
- [ ] Qualify clock corrections and restart anchors without conflicting absolute-
  time mappings. Use scoped-clock tests rather than only comparing rendition dates.
- [x] Signal resumed timestamp generations with consistent EXT-X-DISCONTINUITY
  and EXT-X-DISCONTINUITY-SEQUENCE values, including subtitle playlists.
- [ ] Define in-session timestamp-reset and codec-initialization-change handling;
  do not treat restart tests as coverage for arbitrary dynamic codec changes.

## WebVTT synchronization

- [x] Measure the actual relationship between emitted video PES presentation
  timestamps and ffplayout subtitle timestamps. Account for FFmpeg muxer offsets;
  do not assume that converting milliseconds to 90 kHz alone establishes it.
- [x] Generate X-TIMESTAMP-MAP for each WebVTT segment using that relationship.
  Handle the 33-bit MPEG-TS timestamp wrap and recompute the mapping for a new
  discontinuity generation. Existing resumed segments keep their own mapping.
- [x] Write the header before publishing the segment. Avoid a polling repair job
  or modifications to subtitle segments already available to clients. Defer the
  previously discussed standalone FFmpeg VTT-header adapter until the packager
  prototype establishes whether it is still needed.
- [x] Include every cue intersecting a segment's presentation interval, including
  cues spanning boundaries. Define empty-segment behavior so long subtitle gaps
  do not stall A/V, and retain tolerant handling of malformed source cues.

## Publication, resume and retention

- [x] Publish completed media before atomically publishing playlists that refer
  to it. Generate accurate durations/target durations and stable variant/subtitle
  associations; expose no partial manifests or missing referenced resources.
  Individual files are atomic; a group of playlists is not a filesystem transaction.
- [x] Derive per-rendition RFC 6381 CODECS from H.264 SPS (AVCC/Annex B) and AAC
  initialization (LC/HE/HEv2), including SPS arriving in the first key packet.
  Reject inconsistent/truncated headers and ignore misleading profile metadata.
  Omit declarations when either codec cannot be identified; verify real TS SPS
  against master declarations across three renditions and repeated restarts.
- [x] Preserve usable playlists and referenced segments across normal restarts.
  Continue segment numbering without overwriting cached URLs with different
  bytes, and signal a new timestamp generation where required.
- [x] Reject inconsistent video windows and missing referenced resources without
  overwriting retained media. Preserve the 60-second stale-resume policy; clean
  owned media on backend switches in either direction, leaving unrelated files intact.
- [ ] Finish failure-injection coverage for interrupted multi-playlist publication,
  partial storage failures and incompatible retained codec initialization.
- [x] Retain enough unreferenced media for clients already fetching it. Delete
  only resources owned by this output after its retention period. Preserve
  unrelated outputs and exclusive ownership until the old writer has stopped.
- [x] Bound packet/cue storage and live-window retention; report publication,
  ownership and recovery errors through existing independent destination workers.
- [ ] Qualify slow/full storage and repeated recovery failures end to end: healthy
  outputs must keep running and retries must remain bounded/backed off.

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

- [x] Record accelerated FFmpeg/native CPU/RSS and resume comparisons, packet
  coverage and raw measurements; complete the documented short Safari smoke test.
- [ ] Remeasure short windows after the three-TARGETDURATION retention correction;
  assess PROGRAM-DATE-TIME overhead and qualify the current dependency versions.
- [ ] Differentially compare generated media/manifests with the current backend:
  codec declarations, rendition alignment, seekability, subtitles and A/V sync.
- [ ] Test later player entry, variant switches, subtitle enable/disable, empty
  cues, cross-segment cues, clip transitions, live takeover and return.
- [ ] Test restart/resume, abrupt termination, concurrent destination attempts,
  slow/full storage, missing files and clock corrections with scoped clocks.
- [x] Unit-test X-TIMESTAMP-MAP modulo the 33-bit clock after a timestamp wrap.
- [ ] Simulate continuous playback across 26.5 and 53 hours with video and subtitles
  to cover multiple MPEG-TS wraps; run a separate 24/7 resource/recovery soak.
- [ ] Verify actual playback with our Video.js/hls.js frontend, Safari/native HLS
  and another independent player. Test advertising marker consumers and
  interstitial-capable clients separately; parser round trips are insufficient.
- [x] Keep rollout opt-in, FFmpeg as default, unsupported configurations explicit,
  and backend rollback documented without simultaneous writers to one target.
- [ ] Complete parity/failure-isolation qualification and I/O/end-to-end latency
  measurements before promoting the native backend beyond experimental status.

**First milestone implemented:** a prototype that consumes existing encoded renditions,
produces aligned MPEG-TS segments and atomically published playlists/WebVTT with
verified timestamp mapping, and resumes without unnecessarily deleting media.
This does not close production qualification. Advertising serialization remains
separate follow-up work after broader shared-timeline validation.

## Protocol references

- [RFC 8216: WebVTT and X-TIMESTAMP-MAP](https://www.rfc-editor.org/rfc/rfc8216.html#section-3.5)
- [RFC 8216: PROGRAM-DATE-TIME](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.6)
- [RFC 8216: SCTE-35 in DATERANGE](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.7.1)
- [Apple: Getting Started with HLS Interstitials](https://developer.apple.com/streaming/GettingStartedWithHLSInterstitials.pdf)
