# Experimental native HLS packager

## Architecture decision

Keep encoding and MPEG-TS muxing in FFmpeg. Place HLS orchestration in the
FFmpeg-independent `ff-hls` workspace crate. It receives owned encoded packet
references with explicit PTS, DTS, duration, keyframe status and rendition
identity. A caller-provided segment writer produces the container bytes.

The engine adapter preserves the presentation clock using `mpegts_copyts`,
writes PAT/PMT and marks initial transport continuity counters for each fresh
TS muxer. It closes and renames temporary segment files before the packager
publishes playlists. Segment boundaries use common keyframes across all
renditions and matching audio progress, rather than wall-clock file events.
Nominal targets are multiples of the configured segment length on the shared
program timestamp grid. Joining between grid points produces a shorter first
segment; late cuts do not shift subsequent targets. EXTINF describes the actual
presentation interval. Fresh playlists use the configured segment length as the
fixed TARGETDURATION, with HLS nearest-integer rounding tolerance: for a six-second
target, 6.48 seconds is allowed but 6.5 seconds is rejected before writing media
or draining packets. No extra buffering stage is introduced. Missing boundaries
also remain subject to the existing buffer and wait limits.
Closed-GOP encoders receive forced I-frames on the common program-PTS grid,
so scene cuts cannot shift each rendition onto a different schedule. Frame
references share pixel buffers; forcing picture metadata does not copy pixels.

Existing encoding groups, destination workers and bounded queues are reused.
A slow or failed native destination retains existing output failure isolation.
The native packager adds a bounded compressed segment buffer; it does not start
new encoders or copy uncompressed program frames.

## Selection and rollout

FFmpeg remains the default backend. Each HLS output can select **Native
(experimental)** in its HLS settings. The selection is stored in the existing
output muxer options as `ffplayout_packager: native`, so no database migration
is needed. This application-owned key is consumed before calling FFmpeg.

Changing the selection recreates that destination through normal output
reconciliation, without restarting the channel. Unsupported options are rejected.
There is no automatic backend failover. Concurrent comparisons must use different
directories; compatible engine destinations can share the same encoding source.
The application currently permits one enabled HLS output per channel; concurrent
A/B tests use the engine API, while the web interface switches sequentially.

Both backends retain existing HLS session locks and the 60-second stale-resume
policy for restarts using the same backend. Native resume preserves referenced
resources, retains the configured number of unreferenced resources, continues
filenames and adds a timestamp discontinuity. Native media playlists carry a
packager comment so backend changes can be detected after application restarts.
Switching in either direction removes the destination's previous playlists and
segments under its ownership locks and starts fresh. Unrelated files remain.
Inconsistent video windows or missing resources still fail explicitly.

## Initial scope and limitations

Local MPEG-TS supports H.264/AAC. Local fMP4 supports H.264, HEVC and AV1
with AAC. Both support adaptive renditions and optional WebVTT. The native
packager chooses MPEG-TS for H.264 and fMP4 for HEVC and AV1 by default.
Set `hls_segment_type=fmp4` to use fMP4 with H.264. Changing the container
or changing the fMP4 video codec starts a fresh playlist under the existing
output ownership locks.
Segments are 1–60 seconds, with a configured playlist size of 1–10000 entries.
Short live windows retain additional entries as needed to keep at least three
TARGETDURATIONs of media when removing an entry. TARGETDURATION remains fixed
across resume, including an existing target of eight seconds from older builds. Reducing the segment duration retains the existing target;
increasing it beyond the existing target is rejected without
removing media and requires an explicitly fresh HLS history. The native
backend accepts `hls_flags=program_date_time` and `hls_delete_threshold`
(default 1, minimum 1). The latter keeps that many extra media segments per
track outside the playlist, including after resume; larger windows do not add a
second time-based retention period. Excess files are deleted only after a fresh
playlist has been published. Other custom HLS options and
container metadata are currently rejected.

The native fMP4 writer publishes a separate init file per rendition and
generation. Media playlists attach it to fragments through `EXT-X-MAP`
and retain it while those fragments remain available. WebVTT stays in
separate `.vtt` segments and follows the presentation timeline in either
container.

All media files in a new segment group and its subtitle file are published before
updating their playlists. Each playlist replacement is atomic; the group of
playlist replacements is not. The master is published last. New renditions and
subtitle tracks join at the next media sequence and the current discontinuity
sequence. A re-enabled subtitle track starts a new window if video has advanced;
its stale resources are retired after publication using the configured reserve.
Existing video tracks retain their historical window; conflicting video sequence
endpoints still require deliberate recovery.

The minimal playlist serializer handles the initial scope only. General HLS tag
libraries remain to be evaluated before extending serialization to timed events,
advertising, encryption and LL-HLS.

For fMP4, the writer reads video CODECS identifiers from the init file's
`avcC`, `hvcC` or `av1C` record and the AAC identifier from the muxed track's
codec initialization data. For MPEG-TS, the engine derives per-rendition RFC
6381 CODECS declarations from H.264 SPS data
(AVC configuration records or Annex B) and AAC AudioSpecificConfig, including
explicit/implicit SBR and PS for AAC-LC, HE-AAC and HE-AAC v2. Encoder profile hints
are not substituted for actual headers. If SPS data arrives with the first key
packet, the declaration is supplied before the next master publication. Missing,
conflicting or unsupported initialization leaves CODECS omitted for that rendition;
the packager does not advertise a partial audio/video declaration. This discovery
runs at initialization, not as continuous frame analysis. Dynamic codec changes
within a running destination remain outside the initial scope.
On MPEG-TS resume, declarations are compared with the previous master per
rendition. Changed or unknown declarations are omitted while historical
segments remain in that rendition's live window, then restored automatically.
On fMP4 resume, the first new segment set is checked against the previous init
files. Matching configurations keep the existing window. An init change,
including a resolution change with the same codec, publishes a fresh window
at the next sequence number. Backend and segment-container switches keep the
previous playlists and media available until the replacement renditions are
ready. Replaced segments, WebVTT and init files remain available for the old
playlist duration plus its longest segment. Per-resource expiry markers preserve
that deadline across writer restarts. Replacement playlists continue the media
and discontinuity counters across renditions.

Real encoder tests verify TS timestamps against `X-TIMESTAMP-MAP`, aligned
renditions, restart/resume, switching backends and sharing encoders. Browser/player
qualification, long-running resource measurements, clock-correction behavior and
advertising support remain in `.dev/hls_packager_todo.md`.

## Measurements

See [FFmpeg/native comparison](hls-packager-comparison.md) for the measured CPU,
process RSS, resume time, packet coverage and WebVTT differences, including raw
samples and the reproducible opt-in benchmark. These accelerated tests do not
replace real-time player or long-running qualification.

HLS renditions with identical audio settings share one audio encoder. Within an
output, codec, options, sample rate and time base are common; only differing
rendition audio bitrates create additional encoders. Encoded packets are referenced
into each rendition's existing audio stream, preserving multiplexed TS playlists,
recording selection and outputs that reuse a rendition. Both packager backends and
the direct/threaded encoding paths use this behavior.
