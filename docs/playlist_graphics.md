# Playlist graphics (experimental)

Playlist items accept an optional `graphics` array for static PNGs, animated GIFs and local videos, multiple
layers, opacity/z-index, fades and finite or endless repetitions. Existing
playlists remain valid. No migration or feature flag is required. The frontend
preserves this data on save/load; a visual graphics editor is not implemented.

## Example

Paths are relative to channel storage. This example assumes a 1280×720 program
canvas and `graphics/lower-third.png` inside that storage directory:

```json
{
  "source": "media/clip.mp4",
  "in": 2.0,
  "out": 32.0,
  "duration": 60.0,
  "graphics": [
    {
      "at": 2.0,
      "action": "take",
      "layer": 100,
      "source": "graphics/lower-third.png",
      "size": [1120, 160],
      "position": [80, 520],
      "opacity": 1.0,
      "z_index": 10,
      "fade_in": 0.4,
      "hold": 4.0,
      "fade_out": 0.4,
      "pause": 1.0,
      "repeat": 3
    },
    {
      "at": 20.0,
      "action": "take",
      "layer": 100,
      "source": "graphics/lower-third.png",
      "size": [1120, 160],
      "position": [80, 520],
      "fade_in": 0.4,
      "fade_out": 0.4
    },
    { "at": 22.0, "action": "update", "layer": 100, "opacity": 0.7 },
    { "at": 27.0, "action": "stop", "layer": 100 }
  ]
}
```

The first take runs three complete fade-in → hold → fade-out cycles, with one
invisible second between cycles, ending at 18.4 seconds. The second reuses the PNG, holds until `stop`, then fades out by
27.4 seconds. A layer ID can be taken again after its previous presentation ends
or is cleared. Taking an active ID is invalid; use `update` to change it.

## Actions and fields

All event times/fade/hold durations use nonnegative seconds. Events must be ordered
by `at`; equal-time actions retain declaration order.

| Action | Fields | Effect |
| --- | --- | --- |
| `take` | `layer`, `source`, optional fields below | Start an image/video layer. |
| `update` | `layer`; any of `position`, `opacity`, `z_index` | Change without restarting its fade. |
| `stop` | `layer` | Exit using the take's `fade_out`; repeated stops during that exit do not prolong it. |
| `visibility` | `layer`, `visible` | Hide/show without resetting the timeline. |
| `clear` | `layer` | Remove immediately. |
| `clear_all` | none | Remove clip graphics; keep program logo/text. |

| Optional take field | Default | Meaning |
| --- | --- | --- |
| `position` | `[0, 0]` | `[x, y]` in program-canvas pixels. Negative positions are clipped; YUV420 aligns placement to even pixels. |
| `size` | native size | Prepared `[width, height]`, both even and within 2..8192. Both axes are explicit; a different aspect ratio stretches the image. Odd native PNGs are transparently padded at the right/bottom before YUV conversion/scaling. |
| `opacity` | `1.0` | Base opacity in `[0, 1]`, multiplied once with fade opacity and image alpha. |
| `z_index` | `0` | Higher values draw above lower values; ties sort by layer ID. |
| `fade_in` | `0.0` | Entrance duration. |
| `hold` | omitted | Omitted: hold until stopped/clip end. Number: hold between automatic fades. |
| `fade_out` | `0.0` | Automatic/manual exit duration; zero removes immediately. |
| `pause` | `0.0` | Invisible time between cycles. No pause follows the final cycle; it has no effect with one presentation or an indefinite hold. |
| `repeat` | `1` | Positive total fade-cycle count including the first, or `"forever"`. Repetition requires a finite `hold` and positive cycle duration. |
| `source_repeat` | `"forever"` | GIF/video plays, including the first: a positive integer or `"forever"`. Independent from fade cycles; ignored for static PNGs. |
| `source_end` | `"hide"` | After a finite GIF/video repeat count, hide the graphic or use `"hold_last"` to retain the last image. Ignored for static PNGs. |

For example, `fade_in: 0.5`, `hold: 4`, `fade_out: 0.5`, `pause: 10` and
`repeat: 3` produces three five-second presentations with two ten-second gaps:
35 seconds total. `stop` during a gap removes the layer immediately and cancels
future repetitions. Seeking/pausing preserves the original cycle phase.

Z-index orders clip graphics only. The program logo remains below the scene and
existing configured/runtime text above it. HLS still composites at the largest
rendition, then downscales completed video for smaller variants.

## Timing

`at` is relative to the item's effective start after its media `in` cut. In the
example, `at: 2` corresponds to source-file time 4 seconds. Joining five seconds
into the scheduled item reconstructs its graphics at five seconds; the source
seek and this elapsed item position are tracked separately.

Pause freezes the graphics clock. Skip/restart/replacement discard the occurrence.
Events at/after the shortened clip end are ignored, and graphics never extend
video duration. Endless repetition also ends with its clip. Live takeover hides
clip graphics while preserving established program logo/text behavior. Returning
selects the current scheduled item/phase without replaying missed events.

## Animated sources

```json
{
  "at": 2,
  "action": "take",
  "layer": 101,
  "source": "graphics/lower-third.mov",
  "size": [1120, 160],
  "position": [80, 520],
  "source_repeat": 2,
  "source_end": "hold_last",
  "fade_in": 0.3,
  "fade_out": 0.3
}
```

GIF variable frame durations and video timestamps select the image from elapsed
time since `take`. Source FPS may differ from program FPS. Video audio is ignored.
Use a video codec/container that preserves alpha for transparent graphics;
ordinary video is opaque. Native GIF/video dimensions align down to even pixels;
explicit `size` stretches to that rectangle.

A retake starts the source at zero and reuses cached frames. Update, visibility,
fade repetition and its invisible pauses do not restart or pause the source.
A finite source's `hide` leaves the layer allocated until stop/clear or its
transition ends; clear an active ID before retaking it. A source held on its last
frame can still fade out through `stop`. Sources never extend the scheduled clip.
Seek/live return selects the current item/source phase, without replaying earlier
frames. Small sources keep no decoder after preparation. Larger sources use a background
decoder with bounded lookahead. While it catches up after a seek, the last ready
image remains visible; a newly joined presentation without a suitable ready image
stays hidden until its current phase is available. Selected images are frozen
before output fan-out, so delayed encoders receive the same program frame.

## Preparation, errors and limits

Preparation runs in the background, at most one task per channel. Streaming assets have separate tracked decoder tasks. Clips without
graphics start no task. Video never waits: the scene is absent until ready, and a
late result uses the current phase. Already elapsed graphics do not flash briefly.
A busy preparation slot omits the new occurrence with a warning. Cancellation is
checked between assets and during animation decoding. An in-progress local read or codec call must return before cancellation can complete.
The slot persists across channel restarts. Stop waits up to two seconds for the
tracked job; if it is still finishing, its slot stays occupied so a restart
cannot overlap another preparation job.

Only regular local files within channel storage are accepted, including symlink
containment checks. GIF and video containers MOV/MP4, Matroska/WebM, AVI and MPEG-TS are accepted through restricted file-only demuxing. Remote URLs, nested playlists, animated PNGs, SVG and templates remain unsupported. Existing animated program-logo support is unchanged.

Per occurrence: 256 events, 32 active layers, 64 unique path/size/source-playback combinations,
4096 retained scene-history nodes, and a shared 64 MiB prepared-surface budget.
PNG files are limited to 16 MiB, decoded PNG input/RGBA size to 64 MiB, and dimensions
to 8192. Destination scaling has separate bounded caches. These limits exclude
some temporary decoder/scaler buffers and queued video; they are not an RSS cap.
Animations are cached completely if they fit within 256 frames, five seconds and
64 MiB of prepared pixels (a single held frame can last longer). Larger sources, or sources that encounter shared cache-budget pressure, switch automatically to a tracked
background decoder. Its lookahead targets 500 ms (rounded to source frames), with at most 16 frames/64 MiB
in its decoder cache. A single prepared frame must fit within 64 MiB; reduce
`size` otherwise. Regular local video files are not limited to 64 MiB compressed.
The initial preparation has a cooperative ten-second deadline; it cannot interrupt
a codec call. The scene's 64 MiB admission limit accounts cached sources and
streaming poster frames; dynamic frames and queued references use the shared
256 MiB channel pixel/workspace budget. Decoder internals are outside that budget.

One decoder worker per streaming asset serves up to 32 coalesced presentation
requests, including simultaneous takes with different start times. Worker count
is bounded by the channel's 16 shared preparation-job permits, also used by text,
logos and scene preparation. Decoder workers park when requests are satisfied or
the source is hidden; they resume from current clip time. Clear, fade completion
and live suppression stop obsolete demand. Clip disposal cancels its workers
without joining on the video thread. Channel shutdown joins tracked workers
outside frame processing and retains handles after a timeout. Ready queued frames
remain valid after cancellation. A decoding error retains the last ready image
and logs a warning; it never stops playout.
Repeated takes of the same path/size/source-playback options reuse prepared pixels
within the clip. Different source-repeat/end options currently use separate assets.

A bad/missing/inaccessible or oversized source omits its take and dependent actions when no prior
layer exists, while other valid layers continue. Malformed event configuration
omits that occurrence's graphics. Both warn without stopping media playback.
Invalid optional graphics data stays preserved on playlist save/load so it can
be corrected. Removing `graphics` restores ordinary playback.
