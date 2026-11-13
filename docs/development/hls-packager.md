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
Nominal targets are the initial common keyframe plus successive multiples of the
configured segment length. Late cuts do not move subsequent targets: an 8-second
segment can be followed by 4 seconds for a 6-second setting. EXTINF describes
the actual presentation interval. The fixed TARGETDURATION includes two seconds
of reserve; an interval exceeding it is rejected before writing media or draining
packets. Missing boundaries also remain subject to the buffer and wait limits.
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

Local MPEG-TS with H.264 video, AAC audio, adaptive renditions and optional WebVTT.
Segments are 1–60 seconds, with a configured playlist size of 1–10000 entries.
Short live windows retain additional entries as needed to keep at least three
TARGETDURATIONs of media when removing an entry. TARGETDURATION remains fixed
across resume. Reducing the segment duration retains the existing target;
increasing it beyond the existing target's two-second reserve is rejected without
removing media and requires an explicitly fresh HLS history. The native
backend accepts `hls_flags=program_date_time` and `hls_delete_threshold`
(default 1, minimum 1). The latter keeps that many extra TS/WebVTT segments per
track outside the playlist, including after resume; larger windows do not add a
second time-based retention period. Excess files are deleted only after a fresh
playlist has been published. Other custom HLS options and
container metadata are currently rejected.

All TS files in a new segment group and its subtitle file are published before
updating their playlists. Each playlist replacement is atomic; the group of
playlist replacements is not. The master is published last. New renditions and
subtitle tracks join at the next media sequence and the current discontinuity
sequence. A re-enabled subtitle track starts a new window if video has advanced;
its stale resources are retired after publication using the configured reserve.
Existing video tracks retain their historical window; conflicting video sequence
endpoints still require deliberate recovery.

The minimal playlist serializer handles the initial scope only. General HLS tag
libraries remain to be evaluated before extending serialization to timed events,
advertising, fMP4/CMAF, encryption and LL-HLS.

The engine derives per-rendition RFC 6381 CODECS declarations from H.264 SPS data
(AVC configuration records or Annex B) and AAC AudioSpecificConfig, including
explicit/implicit SBR and PS for AAC-LC, HE-AAC and HE-AAC v2. Encoder profile hints
are not substituted for actual headers. If SPS data arrives with the first key
packet, the declaration is supplied before the next master publication. Missing,
conflicting or unsupported initialization leaves CODECS omitted for that rendition;
the packager does not advertise a partial audio/video declaration. This discovery
runs at initialization, not as continuous frame analysis. Dynamic codec changes
within a running destination remain outside the initial scope.
On resume, declarations are compared with the previous master per rendition.
Changed or unknown declarations are omitted while historical segments remain in
that rendition's live window, then restored automatically. Matching declarations
remain available immediately. This does not clear retained segments; normal
retention still applies, including across repeated restarts.

Real encoder tests verify TS timestamps against `X-TIMESTAMP-MAP`, aligned
renditions, restart/resume, switching backends and sharing encoders. Browser/player
qualification, long-running resource measurements, clock-correction behavior and
advertising support remain in `.dev/hls_packager_todo.md`.

## Measurements

See [FFmpeg/native comparison](hls-packager-comparison.md) for the measured CPU,
process RSS, resume time, packet coverage and WebVTT differences, including raw
samples and the reproducible opt-in benchmark. These accelerated tests do not
replace real-time player or long-running qualification.
