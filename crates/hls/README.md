# ff-hls

Experimental HLS orchestration with no FFmpeg dependency. The library owns
aligned segment boundaries, media/master playlists, WebVTT timestamp mapping,
live-window retention and resume state. Encoders and container muxing belong to
the caller.

`Packager<T, W>` accepts owned `MediaPacket<T>` values on a common microsecond
timeline and a `SegmentWriter<T>`. Each rendition has adjacent video/audio
indices. The writer must retain that presentation clock in the container,
close and publish each segment before returning, and refuse to overwrite
existing files. The engine supplies the MPEG-TS adapter and exclusive output
locks. Calling the crate directly requires equivalent exclusive ownership.

The initial implementation:

- Finds common video keyframes across renditions, then waits for matching audio.
- Holds at most 64 MiB / 32768 compressed packets per destination. Subtitle
  storage is limited to 8 MiB / 10000 cues. An unmatched boundary fails the output.
- Writes media first and atomically replaces each playlist. Multiple playlists
  are not a filesystem transaction; a crash between replacements can leave
  inconsistent windows, which resume rejects without overwriting them.
- Anchors nominal cuts at the first common keyframe plus multiples of the
  configured segment duration. A delayed cut can be followed by a shorter segment
  to catch up, while EXTINF always describes the actual interval.
- Uses a fixed target duration, with two seconds of keyframe tolerance.
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
  and avoids reusing even orphaned segment filenames. Unreferenced resources are retained up to
  `delete_threshold` per track (normally 1), then deleted after playlist publication.
- Leaves playlists live when finishing. The engine owns stale-playlist policy.

The engine's initial adapter supports local MPEG-TS, H.264/AAC and up to 16
renditions. fMP4/CMAF, LL-HLS, encryption, advertising markers and remote upload
are follow-ups. Unsupported options are rejected. FFmpeg remains the default.

Run `cargo test -p ff-hls` for container-independent tests. Engine integration
tests prefixed with `native_hls` exercise real encoders, PES timestamps, variants,
subtitles, resume and encoder sharing.
