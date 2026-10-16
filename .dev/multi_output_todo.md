# Multiple outputs per channel

Status: output instances and the consolidated editor are implemented. See
[Development Notes](README.md) and [output documentation](../docs/output.md).

## Implemented

- One playlist, live takeover, program timeline, overlays and audio processing.
- Independent destination workers and bounded frame/packet queues.
- Explicit encoding-source selection for Stream, MoQ and HLS. Own encoding is
  independent; selected output/variant dependencies share one original profile.
- Adaptive HLS/WebVTT sources and named renditions support packet reuse, including
  derived HLS outputs. Codec/container compatibility and cycles are validated.
- Packet-copy recording can consume original or derived outputs. Dedicated
  recording encoding remains separate. Audio-only sharing across resolutions is deferred.
- Multiple destinations of the same type; up to eight configured instances.
- One desktop and one regular MoQ output, including MoQ as the only output.
- Configured but disabled instances retain their names and settings.
- Separate Outputs tab with stacked cards and collapsible advanced options, runtime status and
  per-instance names, activation, encoding and destination settings.
- General settings replace the Playout tab; Ingest has its own tab, and Outputs
  appears below Recording.
- MoQ is shown first and, when enabled, supplies the preview in the Player window.
- Shared program resolution and FPS under General → Processing; individual output resolutions
  or inherited program resolution. Different output FPS are deliberately out
  of scope: all outputs use the program frame rate.
- MoQ is built in, with a shared Vue player for internal and public playback.
  Public access is off by default per output, protected by channel-scoped relay
  admissions; changing permission revokes existing anonymous sessions within five seconds.
- MoQ is an ordinary output row. The old preview table, API field and runtime
  branch have been removed; no separate `config_moq` table is needed.
- Codec names are not restricted by SQL. MoQ exposes FFmpeg encoders supported
  by the current importer, based on codec IDs; AV1 still needs a supported import path.
- CRF, preset and tune settings for x264/x265; zerolatency remains the default.
- All new database changes are consolidated in `00007_output_instances.sql`.
- CLI null output remains temporary and does not replace persisted settings.
- New timing diagnostics use Trace; diagnostic timestamp collection and
  statistics are gated when Trace is disabled. Operational pacing and bounded
  queue timing remain active. Historical log levels are unchanged.

## Failure and restart policy

Opening, transport, muxing and compressed queue failures isolate the affected
destination. Reconnection retries every 15 seconds for up to three minutes from
the first failure; successful media delivery resets the complete failure budget.
After exhaustion, the Outputs page can reconnect just that destination. All
encoding groups and the program continue while destinations are offline. Recovery
uses current media from the next keyframe, with bounded queues. Encoder failure or
raw queue overflow still requires a channel restart for the group. Output settings
and activation changes still require a channel restart. Desktop owns program
pacing when present. Recording remains a separate feature.

The old auxiliary MPEG-TS path and its `mpegts-output` Cargo feature were removed;
MoQ uses the common destination pipeline. Optional timing diagnostics remain.

## Later work

- Change or activate individual outputs without restarting the channel.
- Consider sharing audio alone when video resolutions differ.
- Multiple MoQ instances per channel (separate broadcast identities).

Dynamic output configuration remains deferred. Different FPS per output are not planned.

## Validation still needed

- A sustained multi-output run covering clip transitions, live takeover,
  slow/disconnected destinations and long-term audio/video timing.
- Browser A/V playback with the new multi-output runtime. The Outputs page has
  been visually checked in Firefox without saving or changing the running channel.
- Sustained recovery checks with external RTMP/SRT servers in addition to the local TCP tests.

Existing automated tests cover isolated output failure, bounded queues,
multiple resolutions, shared HLS/MPEG-TS/FLV encoding, identical packet payloads,
packet-copy recording, real mid-stream TCP disconnects, clip transitions and
desktop fanout. A production-interval TCP reconnect test also verifies that a
second shared output retains all 450 frames with continuous timestamps.
The earlier user endurance log predates the multi-output implementation.

## Encoding source selection

- One source selector replaces the sharing checkbox for MoQ, Stream and HLS.
- Own encoding shows private settings; selecting an output/variant hides them.
- Saved enabled sources are offered subject to codec compatibility and cycle checks.
- Source chains resolve to the original profile/rendition. Identical independent
  settings do not implicitly share an encoder.
- HLS derivatives publish one rendition. Original and derived outputs remain
  available as recording sources.
- Source/variant fields and compatibility with the former sharing flag use the
  existing migration 0007; no new database schema change is needed.
- MoQ importer health drives runtime status. The editor refreshes every five seconds.
