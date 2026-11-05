# Playlist graphics (experimental)

Playlist items accept an optional `graphics` array for static PNG/AVIF images, animated GIF/AVIF sources and local videos, multiple
layers, opacity/z-index, fades and finite or endless repetitions. Existing
playlists remain valid. No migration or feature flag is required. The frontend
provides an experimental action form in the playlist clip editor and preserves
this data on save/load. Its position canvas supports proportional resizing,
center snapping and a transparency checkerboard. Independent manual live layers
are available on the [Graphics page](graphics_control.md).

## Frontend editing

On the player page, edit a playlist item and use its **Graphics** section:

1. Add actions and select their time (seconds after the clip's effective start),
   action type and layer ID. The file browser selects local channel-storage paths.
2. Configure take position/size, opacity/z-index, fades, optional total duration,
   repeat pauses, fade-cycle count and independent animated-source repeat/end policy.
   Source resolution and duration are shown when available. High quality scaling
   is enabled by default and can be disabled per take.
   Update actions change only checked properties. Stop fades out; Clear removes
   immediately; Visibility changes visibility without restarting source time.
3. Keep actions ordered by time. Move buttons allow explicit equal-time ordering;
   **Sort by time** preserves declaration order for equal timestamps.
4. Confirm the clip dialog, then save the playlist with the existing playlist Save
   button. To test the saved scene, start/select the clip again. The media-file
   preview does not render program graphics and this form does not send live Take
   commands.

The form checks numeric fields, repetition rules, event/layer limits and layer
lifecycle/order before accepting edited graphics. File access, decoding, pixel
budgets and the full compiled scene remain backend checks. Actions beyond the
clip end are retained with a warning and ignored in playback.

Initially invalid or unsupported JSON stays intact when editing other clip settings.
An initially valid graphics timeline is revalidated when In/Out changes: extending
its effective duration can activate previously ignored, conflicting actions and
must be corrected before confirmation. Ignored actions keep their JSON field/type
and time-order checks, but their playback settings (such as fade-repeat/hold rules)
are not validated until they fall inside the clip. Unknown fields on recognized
events are retained. The JSON disclosure is read-only;
**Remove all graphics** explicitly clears the stored actions. Cancelling the clip
editor discards edits. Changing channels closes the old channel's draft.

Split clips before adding graphics. Automatic clip splitting is disabled for
items with graphics; copying the same clip-local event list into every part would
change its meaning. While split cuts are being prepared, the graphics form is
disabled. A separate timeline/state reconstruction adapter is needed to combine
those operations safely.

The reusable `PlaylistGraphicsEditor` receives JSON, effective duration and a
file-browser loader; it does not own playlist stores or HTTP graphics controls.
Run its adapter/lifecycle checks with `npm run test:graphics` (Node 24+).

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
      "duration": 4.8,
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
| `high_quality` | `true` | Alpha-correct source scaling for cleaner edges. Disable for faster straight-alpha scaling. Premultiplied sources are normalized in both modes. Applies when preparing a new take/clip. |
| `opacity` | `1.0` | Base opacity in `[0, 1]`, multiplied once with fade opacity and image alpha. |
| `z_index` | `0` | Higher values draw above lower values; ties sort by layer ID. |
| `fade_in` | `0.0` | Entrance duration. |
| `duration` | omitted | Total duration of one fade cycle, including fade-in and fade-out. Shorter durations shorten both fades proportionally. Without `duration` or `hold`, fade out before clip end (or an earlier finite source end). |
| `hold` | omitted | Legacy stand time between fades, excluding fade-in and fade-out. Use either `duration` or `hold`, never both. |
| `fade_out` | `0.0` | Automatic/manual exit duration; zero removes immediately. |
| `pause` | `0.0` | Invisible time between cycles. No pause follows the final cycle; it has no effect with one presentation or an indefinite hold. |
| `repeat` | `1` | Positive total fade-cycle count including the first, or `"forever"`. Repetition requires an explicit `duration` or `hold` and positive cycle duration. |
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
A finite source with `hide` fades out before its final repetition ends, using
the source duration metadata until the decoder knows the exact duration.
The earlier of the clip end and finite source end also caps explicitly timed
presentations. A boundary inside an active fade cycle shortens that final cycle;
earlier cycles and invisible repeat pauses retain their timing. If the remaining
cycle is shorter than fade-in plus fade-out, both fades shrink proportionally,
so a short source can still reach full opacity. Stop evaluates that fitted
curve and cannot brighten the graphic or extend it beyond the boundary. Its
fade-in starts at `take`, just as for static images. For example, `duration: 10`,
`fade_in: 1` and `fade_out: 1` give one second of fade-in, eight seconds at full
opacity and one second of fade-out. A finite source's `hide` leaves the layer
allocated until stop/clear or its transition ends; clear an active ID before
retaking it. A source held on its last
frame can still fade out through `stop`. Sources never extend the scheduled clip.
Seek/live return selects the current item/source phase, without replaying earlier
frames. Small sources keep no decoder after preparation. Larger sources use a background
decoder with bounded lookahead. Sources known to exceed five seconds switch to
streaming after their initial frames, without decoding the entire cache window
before publication. During the initial fade-in, the prepared first image can
already fade in while the decoder catches up. After that entrance, a new join
without a suitable image stays hidden until its current phase is available;
existing presentations retain their last ready image during a seek. Selected images are frozen
before output fan-out, so delayed encoders receive the same program frame.

## Preparation, errors and limits

Preparation runs in the background with at most two active scene-preparation jobs
per channel and one waiting occurrence. A new clip cancels obsolete jobs and
replaces any waiting occurrence. If the previous job is still returning from
non-interruptible local I/O, the spare worker can prepare the current clip.
Both workers share the channel's pixel, resource and decoder budgets. Clips
without graphics create no job and discard any obsolete waiting occurrence.

Playback never waits for preparation. A delayed result joins at the current clip
time, without replaying missed events; results for departed clips are discarded.
Stopping cancels both active jobs and drops the waiting occurrence immediately.
Tracked jobs have a shared two-second join deadline. A job still finishing keeps
its handle and worker slot across restarts; restarting cannot exceed two active
scene-preparation jobs. Disposal cancels tracked work and releases the dispatcher;
blocking jobs retain their resource reservations until they actually exit.
Streaming assets have separate tracked decoder tasks.

Only regular local files within channel storage are accepted, including symlink
containment checks. GIF, AVIF and video containers MOV/MP4, Matroska/WebM, AVI and MPEG-TS are accepted through restricted file-only demuxing. Remote URLs, nested playlists, animated PNGs, SVG and templates remain unsupported. Program logos use the same source preparation; see [supported alpha formats](animated_logos.md).

Per occurrence: 256 events, 32 active layers, 64 unique path/size/source-playback combinations,
4096 retained scene-history nodes, and a 256 MiB scene admission budget, also subject to the shared channel budget.
PNG files are limited to 16 MiB, decoded PNG input/RGBA and prepared PNG storage
to 64 MiB, and dimensions to 8192. Destination scaling has separate bounded caches. These limits exclude
some temporary decoder/scaler buffers and queued video; they are not an RSS cap.
Animations are cached completely if they fit within 256 frames, five seconds and
the configured per-source animation-cache ceiling (default 128 MiB; a single held frame can last longer). Larger sources, or sources that encounter shared cache-budget pressure, switch automatically to a tracked
background decoder. Its lookahead targets 500 ms (rounded to source frames), with at most 16 frames and the configured per-source byte ceiling
in its decoder cache. A single prepared frame must fit within that ceiling; reduce
`size` otherwise. Regular local video files have no 64 MiB compressed-file limit.
The initial preparation has a cooperative ten-second deadline; it cannot interrupt
a codec call. The scene's 256 MiB admission limit accounts cached sources and
streaming poster frames; dynamic frames and queued references use the shared
256 MiB channel pixel/workspace budget. Decoder internals are outside that budget.

**Graphics → Settings → Animation cache (MiB)** configures
32–1024 MiB per source. This is allocated on demand and does not enlarge the
shared 256 MiB channel budget. Changes apply to newly loaded graphics; Take/Start
reloads an existing manual layer, and the next clip prepares playlist graphics
with the new limit. The program logo reloads in the background. No playout restart
is required. See [graphics control](graphics_control.md) for persistence and API details.

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
Scheduled takes, visibility entrances and fade repeats request their first visible
source position up to 500 ms in advance. During that short window, a streamed
source retains its bounded decoded lookahead; the first visible program frame
selects the matching source image from it, even with different source/program
FPS. This does not advance the take or restart the animation clock. Outside the
window, longer invisible intervals still park the decoder and release its cache.
Current visible presentations take precedence over a future retake on the same
layer. A seek/live return or late asset preparation can bypass the lead window;
in that case the established ready/poster/last-good behavior remains in effect.
No new worker or pixel copy is added by this scheduling step; lookahead references
share the existing channel-budgeted frames. It is not a hard decoding deadline.

Repeated takes of the same path/size/source-playback options reuse prepared pixels
within the clip. Different source-repeat/end options currently use separate assets.

A bad/missing/inaccessible or oversized source omits its take and dependent actions when no prior
layer exists, while other valid layers continue. Malformed event configuration
omits that occurrence's graphics. Both warn without stopping media playback.
Invalid optional graphics data stays preserved on playlist save/load so it can
be corrected. Removing `graphics` restores ordinary playback.
