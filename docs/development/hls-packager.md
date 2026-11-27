# Native HLS packager

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
The native packager sizes its compressed segment buffer from the rendition bandwidth,
packet rate and segment duration, up to 256 MiB and 262144 packets per output; it does not start
new encoders or copy uncompressed program frames.

Each native HLS output can buffer up to 256 MiB of compressed packet payload
and 262144 packets. Concurrent outputs contribute independently, and packet
metadata, subtitles, and muxer allocations add to this memory. The estimate
uses two segment durations. At sustained high bitrate, a delayed common keyframe
can exhaust the budget before the separate boundary timeout (three segment
durations plus five seconds of media time).

## Selection and rollout

New HLS outputs default to **Native**. HLS outputs with no saved backend
selection use **FFmpeg**. Both backends are selectable in the HLS settings.
The application stores the selection in the existing muxer options as
`ffplayout_packager: native` or `ffplayout_packager: ffmpeg`, and consumes
this key before calling FFmpeg.

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
(default 1, minimum 1). The threshold keeps a minimum reserve of unreferenced
media segments per track. An evicted segment also gets a grace period based on
its duration and the longest observed playlist window. Retention markers carry
the expiry across restarts. Expired media are eligible for deletion after a
successful publication. Other custom HLS options and container metadata are
currently rejected.

The native fMP4 writer publishes a separate init file per rendition and
generation. Media playlists attach it to fragments through `EXT-X-MAP`
and retain it while those fragments remain available. WebVTT stays in
separate `.vtt` segments and follows the presentation timeline in either
container.

The packager publishes the complete media set, then stages all manifests before
replacing any of them. Each manifest has its own atomic replacement, with the
master published last. A process interruption between replacements can leave
different tracks at different sequences. Cleanup failures are logged and retried
without interrupting publication. When a filesystem does not support directory
sync, publication continues with a warning. File sync and other I/O failures
still fail the write. New renditions and
subtitle tracks join at the next media sequence and the current discontinuity
sequence. A re-enabled subtitle track starts a new window if video has advanced;
its stale resources are retired subject to the grace period and reserve.
When a manifest references missing media, the packager publishes a fresh
window with a discontinuity once a complete new segment set is ready.
Conflicting sequence assignments or invalid manifests still require recovery.

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

With native MPEG-TS or fMP4 and `hls_shared_audio=1`, the packager writes the first
rendition's AAC packets to one audio-only playlist and refers to it from all
video variants. Each segment set uses the common video keyframe cut. The audio
playlist uses the actual AAC sample start and duration, and all fragments and
WebVTT use the same presentation-clock offset. A layout change starts a new
window under the same output ownership locks.
