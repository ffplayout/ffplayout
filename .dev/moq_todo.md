# MoQ subtitles

## WebVTT text tracks

- [ ] Reuse the existing WebVTT sidecar reader and cue timing from the HLS
  pipeline. Clip cues to the actual played interval and map them to the
  program timeline.
- [ ] Publish timed cues as a selectable text rendition in the Hang catalog.
  Use the same time origin as the MoQ audio and video tracks and include the
  configured language and label.
- [ ] Send cues in time for browser playback and make an already active cue
  available to viewers who join or reconnect during it. End cues at clip
  boundaries and cover skips, seeks and transitions to and from live ingest.
- [ ] Verify track selection, subtitle toggling and cue timing in the existing
  MoQ browser player. Test channels without sidecars and concurrent HLS and
  MoQ output.

See [Hang text tracks](https://doc.moq.dev/concept/hang) and the
[ffplayout MoQ documentation](../docs/moq.md).
