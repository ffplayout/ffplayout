# ff-hls

HLS orchestration with no FFmpeg dependency. The library owns
aligned segment boundaries, media/master playlists, WebVTT timestamp mapping,
live-window retention and resume state. Encoders and container muxing belong to
the caller.

`Packager<T, W>` accepts owned `MediaPacket<T>` values on a common microsecond
timeline and a `SegmentWriter<T>`. Each rendition has adjacent video/audio
indices. The writer must retain that presentation clock in the container,
close and publish each segment before returning, and refuse to overwrite
existing files. The engine supplies the MPEG-TS and fMP4 adapters and exclusive output
locks. Calling the crate directly requires equivalent exclusive ownership.

The initial implementation:

- Finds common video keyframes across renditions, then waits for matching audio.
- Derives the compressed packet budget per destination from segment duration,
  aggregate rendition bandwidth, frame rate and audio track count. Budgets range
  from 64 to 256 MiB and 32768 to 262144 packets. These are limits on queued
  payload and packet count. Packet metadata and muxer memory are additional,
  and the budgets of concurrent destinations add up. Subtitle storage is
  limited to 8 MiB / 10000 cues. The budget targets two segment durations and
  may be exhausted before the independent missing-boundary check triggers.
- Writes media first and atomically replaces each playlist. Multiple playlists
  are not a filesystem transaction; a crash between replacements can leave
  different last sequences. On restart, lagging tracks rejoin at the next unused
  sequence with a discontinuity. Cached playlists retain access to their media.
- Anchors nominal cuts on the shared program grid at multiples of the
  configured segment duration, with a shorter first segment when joining between grid points. A delayed cut can be followed by a shorter segment
  to catch up, while EXTINF always describes the actual interval.
- Uses the configured segment length as the fixed target duration for fresh
  playlists, with nearest-integer rounding tolerance (6.48 s fits a 6 s target;
  6.5 s does not). No additional buffering stage is required.
  An overlong interval fails before writing media or consuming buffered packets.
  The caller must supply a shared keyframe grid at least every two seconds.
  New tracks join the next sequence without requiring historical segments.
- Emits cues in every segment they overlap and maps milliseconds to the common
  90 kHz clock modulo 2^33. Empty subtitle segments are also written.
- Publishes per-rendition CODECS declarations supplied by the caller, including
  initialization discovered before the first segment is published. The engine
  derives H.264/AAC declarations from actual headers and omits uncertain values.
  On resume, changed or unknown declarations stay omitted until historical media
  leaves the live window; retained segments are not cleared.
- Keeps at least three target durations in a rolling live window before evicting
  entries, even when the requested entry count is smaller. The configured
  unreferenced-segment reserve applies after this playable window.
- Preserves TARGETDURATION across resume, including when segment duration is
  reduced. An increase requiring a larger target is rejected without deleting
  existing media; it requires an explicitly fresh HLS history.
- Resumes existing local playlists, adds a discontinuity for the new generation,
  and avoids reusing even orphaned segment filenames. If a referenced file is
  missing, a complete new segment set replaces the old history with a discontinuity.
  Evicted media remain available for their segment duration plus the longest
  observed playlist window. The `delete_threshold` also keeps a minimum reserve
  of unreferenced segments per track. Cleanup errors are reported separately
  from publication and retried on subsequent segments.
- Leaves playlists live when finishing. The engine owns stale-playlist policy.

The engine supports local MPEG-TS with H.264/AAC and local fMP4 with H.264,
HEVC, or AV1 and AAC, with up to 16 renditions. LL-HLS, encryption, advertising
markers and remote upload are follow-ups. Unsupported options are rejected.
New HLS outputs default to Native. Existing outputs without an explicit
packager selection continue to use FFmpeg.

Run `cargo test -p ff-hls` for container-independent tests. Engine integration
tests prefixed with `native_hls` exercise real encoders, PES timestamps, variants,
subtitles, resume and encoder sharing.
