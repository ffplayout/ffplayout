# HLS packager backlog

Native orchestration of HLS segments, playlists and WebVTT, with timed
advertising events planned separately. The opt-in implementation lives in `crates/hls`;
see [Development Notes](README.md). Keep the existing FFmpeg HLS path available
until a replacement passes compatibility and long-running tests.

Status reconciled on 2026-11-17. Checked items describe implemented behavior or
explicitly recorded qualification evidence, not unrestricted production approval.
The current core suite has 29 tests. The historical Linux comparison used the
earlier 17-test suite; the current failure tests were run locally on macOS.
See the release qualification notes below for real-stream and browser evidence.

## Initial implementation status

- [x] Add the FFmpeg-independent `ff-hls` crate and the engine's MPEG-TS adapter.
- [x] Add per-output FFmpeg/native selection, defaulting to FFmpeg, using existing
  persisted muxer options. A change recreates only that output.
- [x] Prototype aligned H.264/AAC variants, WebVTT with X-TIMESTAMP-MAP, atomic
  individual playlist publication and bounded packet storage.
- [x] Test real encoders, resume, fresh starts on backend switches and sharing
  existing encoders with the FFmpeg backend.
- [x] Keep nominal cuts on the shared program timestamp grid; test late/missing keyframes,
  mismatched rendition boundaries, packet preservation and crossing captions.
- [x] Repeat the 30-run FFmpeg/native comparison and run the 17 core tests on
  Linux `/dev/shm`; complete short paced runs and a Safari native-HLS smoke test.
  Current real-stream soak evidence and remaining qualification are recorded below.
- [x] Use configured segment length as fresh TARGETDURATION, with nearest-integer
  rounding tolerance and a shorter first segment when joining the program grid.
  Reject real overruns before media publication; keep existing targets on resume.
  Tests cover real H.264/AAC entry at off-grid timestamps at 25/30/50 fps,
  independent segment decoding, native live takeover/pause/return through three
  encoders, and a later overrun preserving published history through recovery.
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
- [x] Test storage-full errors through the container-writer interface, failed
  subtitle/video playlist publication after media completion, and inconsistent
  variant windows on resume. Preserve history and avoid orphan filename reuse.
- [x] Exit a real subprocess without destructors after writing the second segment
  set but before publishing its playlists; verify safe resume and retained bytes.
- [ ] Extend failure injection to incompatible retained codec initialization and
  a process kill during actual FFmpeg container writes.
- [x] Retain enough unreferenced media for clients already fetching it. Delete
  only resources owned by this output after its retention period. Preserve
  unrelated outputs and exclusive ownership until the old writer has stopped.
- [x] Bound packet/cue storage and live-window retention; report publication,
  ownership and recovery errors through existing independent destination workers.
- [x] Test a blocked destination sink alongside a healthy file output worker;
  verify healthy delivery completes before releasing the blocked write.
- [x] Test unavailable native HLS storage alongside a healthy MPEG-TS output;
  verify retry exhaustion and all healthy video packets. Existing retry tests
  cover spacing, deadlines, repeated outages and manual recovery.
- [x] Fill a real 64-MiB macOS HFS+ RAM disk during paced native HLS delivery.
  Keep a MPEG-TS output on the same encoder running, preserve published history
  through ENOSPC and recover using the normal retry after freeing space.
- [x] Exercise real delayed filesystem I/O in Podman: synchronously mounted ext4
  over dm-delay, shared encoder, a healthy MPEG-TS destination, queue overflow
  isolation and automatic recovery after removing the delay.
- [ ] Repeat slow/full-volume recovery with multiple HLS renditions and sustained
  throughput/IOPS limits; current filesystem runs use one rendition and one shared output.

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
- [x] Smoke-test later player entry, a two-minute backward seek and subtitle
  disable/re-enable in Firefox/Video.js and Safari/native HLS against the real stream.
- [x] Inspect live takeover/return segments and empty-to-populated VTT transitions
  against the running stream, in addition to automated encoder tests.
- [ ] Extend browser coverage to sustained rendition switching, cross-segment
  cues and clip/live transitions observed directly in each player.
- [ ] Test restart/resume, abrupt termination, concurrent destination attempts,
  slow/full storage, missing files and clock corrections with scoped clocks.
- [x] Unit-test X-TIMESTAMP-MAP modulo the 33-bit clock after a timestamp wrap.
- [x] Inspect a real 58 h 50 min playlist run and video/WebVTT samples after two
  MPEG-TS clock wraps; validate timestamp mapping and decodability.
- [ ] Observe uninterrupted browser playback across the wrap boundaries and run
  a separate resource/recovery soak with repeated destination failures.
- [x] Verify real-stream playback using the frontend's installed Video.js 10.4.0
  `hlsjs-video` media component in Firefox 160 and native HLS in Safari 27.2.
- [ ] Verify another independent player and the complete frontend controls.
  Test advertising/interstitial consumers separately when implemented.
- [x] Keep rollout opt-in, FFmpeg as default, unsupported configurations explicit,
  and backend rollback documented without simultaneous writers to one target.
- [ ] Complete parity/failure-isolation qualification and I/O/end-to-end latency
  measurements before promoting the native backend beyond experimental status.

**First milestone implemented:** a prototype that consumes existing encoded renditions,
produces aligned MPEG-TS segments and atomically published playlists/WebVTT with
verified timestamp mapping, and resumes without unnecessarily deleting media.
This does not close production qualification. Advertising serialization remains
separate follow-up work after broader shared-timeline validation.

## Release qualification evidence (2026-11-17)

- Real stream: `https://stream3.daswort.tv/live/master.m3u8`, v2.9.0-alpha.2.
  Local `tmp/log.log` spans 2026-11-14 21:42 to 2026-11-17 08:32
  (58 h 50 min), with one start, no errors and two subsecond playlist-gap
  warnings. Clip deltas range from -0.080 to +0.436 seconds. RSS grows, while
  periodic glibc used memory remains roughly 112–118 MiB; keep monitoring.
- All three video renditions and subtitles had aligned 600-segment / 60-minute
  windows, six-second EXTINF/TARGETDURATION and matching PROGRAM-DATE-TIME.
  Thirteen TS samples decoded without errors; three VTT mappings matched video
  presentation timestamps after two wraps. This is sampling, not continuous
  browser observation across either wrap.
- The subsequently supplied live log covers takeover at 08:41:10 and return at
  08:53:10. Thirty TS samples around both transitions decoded without errors;
  each had 150 video frames / six seconds and a starting keyframe. Rendition
  clocks stayed aligned, and ten VTT mappings matched video. Empty live captions
  were followed by playlist captions after return. glibc used memory returned
  to approximately 31 MiB after live ended. Timestamp alignment alone does not
  establish perceptual lip sync.
- Automated regression coverage: `cargo test -p ff-hls` (29 tests), including
  injected StorageFull, two filesystem publication boundaries and abrupt child
  exit. `cargo test -p ff-engine --no-default-features --lib
  output::delivery::tests::` (14 tests) covers actual delivery workers, a blocked
  sink, unavailable native HLS storage, retry exhaustion and healthy output.
  The 15 real-encoder/recovery tests in `output::encoded::recovery_tests::`
  also passed. These controlled simulations are complemented by the real
  full-filesystem test below; slow-device and broader stress coverage remain open.
- Real full-filesystem qualification: the opt-in
  `output::parallel::storage_tests::full_filesystem_preserves_history_and_shared_encoder_output_then_recovers`
  test ran on a dedicated 64-MiB HFS+ RAM disk on macOS. After four seconds of
  real native HLS publication, writing roughly 62 MiB of filler produced OS
  error 28 (ENOSPC). Published playlists and their referenced segment bytes
  stayed unchanged while full. A second MPEG-TS destination shared the same
  encoder and retained all 650 video packets over 26 seconds. Removing the
  filler allowed automatic HLS recovery with the normal 15-second retry interval;
  recovered playlists referenced readable segments containing video.
  The RAM disk was detached after qualification. This tests capacity exhaustion,
  not a slow device or power-loss durability.
  Reproduce only on a dedicated disposable filesystem: set
  `FFPLAYOUT_STORAGE_TEST_ROOT` to its mount directory and create the marker
  `.ffplayout-storage-test` there, then run
  `cargo test -p ff-engine --no-default-features --lib full_filesystem_preserves -- --ignored --nocapture`.
  The test fills that filesystem, caps filler writes at 96 MiB and stays ignored
  in normal CI. Its healthy output is written outside the supplied filesystem.
- Slow-filesystem qualification ran in rootless Podman on the Fedora 44 ARM64
  Lima VM (Linux 6.19.10), using Debian FFmpeg 7.1.5 with libx264 and Rust 1.100.
  A dedicated 256-MiB loop image was mapped through dm-delay and mounted as ext4
  with `sync,noatime`. Read latency stayed unchanged; write/flush delays changed
  from 0 to 20 to 500 ms and back to 0 while the real encoder ran.
  A 4-KiB write plus fsync measured about 4–7 ms without delay, 164–170 ms at
  20 ms and 5,985–6,013 ms at 500 ms (filesystem operations involve multiple block requests).
  The strong delay caused the HLS destination queue to overflow; the shared
  MPEG-TS output retained all 1,500 video packets over 60 seconds. HLS recovered
  after the delay was removed. The opt-in `slow_filesystem_keeps_shared_output_running_and_recovers`
  test also checks that recovered HLS reaches the last program second. Both
  60-second runs passed; the final run reached the last video frame at 59.960 s.
  The controller uses `FFPLAYOUT_STORAGE_CONTROL` markers `started`, `slow-active`
  and `restored`; changing the block mapping stays outside production code.
  Local controller scripts and measurements are in `tmp/hls-slow-*.sh`,
  `tmp/hls-slow-run.py`, `tmp/podman-slow-hls.log` and
  `tmp/podman-slow-hls-timing.json`. Only the dedicated image/device and test
  containers are used; the test filesystem and device are removed afterwards.
- Safari 27.2/native HLS: late entry, advancing playback, 120-second backward
  seek and subtitle off/on completed with no media errors; 16 active cue texts
  were observed and subtitles were visually confirmed. Continued playback was
  checked for 30 seconds after re-enabling captions (approximately 75 seconds total).
- Firefox 160/Video.js 10.4.0: the same sequence completed using the installed
  frontend `hlsjs-video` component. Active cues continued after seek, subtitle
  re-enable and both 720p -> 240p -> 720p switches; the final approximately
  100-second run observed 28 active cue texts and no media errors. Browser
  test artifacts are local under
  `tmp/hls-browser-*.json`; the test page is `tmp/hls-browser-qualification.html`.
  This exercises the media component, not the complete Vue application/skin.
- The tests run muted: packet timing and visible captions are checked, but
  perceptual audio quality/lip sync still requires listening. Neither browser
  smoke tests nor the successful soak promote Native to the default backend.

## Protocol references

- [RFC 8216: WebVTT and X-TIMESTAMP-MAP](https://www.rfc-editor.org/rfc/rfc8216.html#section-3.5)
- [RFC 8216: PROGRAM-DATE-TIME](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.6)
- [RFC 8216: SCTE-35 in DATERANGE](https://www.rfc-editor.org/rfc/rfc8216.html#section-4.3.2.7.1)
- [Apple: Getting Started with HLS Interstitials](https://developer.apple.com/streaming/GettingStartedWithHLSInterstitials.pdf)
