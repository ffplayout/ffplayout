# Animated logos

Logo settings are managed under **Graphics → Program Logo** and apply while the
channel is running, including during live ingest. See
[Graphics control](graphics_control.md) for the page, permissions and save behavior.

The existing logo path accepts a static image, animated GIF, or local video file.
Animated sources repeat indefinitely, ignoring a file's finite repeat count.
Animation uses the existing logo path and the quality/cache controls described
below. Existing logo scale, position,
opacity and fades apply. Paths remain relative to channel storage in the frontend.

GIF frame delays may differ from one image to the next. Video presentation
timestamps and frame durations determine playback speed, independently of the
program frame rate. Low-rate sources repeat images; high-rate sources skip images.
There is no interpolation or optical-flow processing. Exact rational loop duration
is retained even for fractional rates such as 30000/1001 fps, avoiding a cumulative
rounded-duration error at loop boundaries.

## Formats and alpha

Regression fixtures cover:

- GIF with variable frame delays, transparency and embedded infinite looping.
- QuickTime Animation (`qtrle`, ARGB in `.mov`) with alpha.
- A 30000/1001 fps QuickTime Animation video.
- Opaque FFV1 video in Matroska, including bounded streaming and repeated loops.
- ProRes 4444 and FFV1 with transparent, semi-transparent and opaque regions.
- VP8/VP9 WebM with `alpha_mode=1`, including transparent and semi-transparent pixels.
- AVIF with separate 8-/10-bit alpha images, and animated AVIF with timed alpha tracks.
- Existing static PNG logos, including a full 3840×2160 graphic.

Other image/video formats depend on the FFmpeg decoders in the build. They are
not all guaranteed animation/alpha formats. An ordinary video without alpha is
an opaque rectangle; its audio is ignored. There is no automatic chroma-keying.
Animated WebP, APNG and image sequences are not part of the tested initial scope.
Use a regular local file, not a network URL, live stream or pipe.

Transparent WebM graphics require FFmpeg's `libvpx` (VP8) or `libvpx-vp9` (VP9)
decoder. ffplayout selects it for graphics streams marked `alpha_mode=1`; an
initial `yuv420p` pixel-format report does not mean that the alpha channel is
absent. Without the required decoder, source preparation reports an error
instead of silently displaying an opaque rectangle. Ordinary video decoding and
unmarked VP8/VP9 sources retain their default decoder selection. The same behavior
applies to manual and playlist graphics.

HEVC auxiliary-layer alpha was verified with the native HEVC decoder in the
tested FFmpeg 9-era build. Support depends on the linked FFmpeg version; an
initial `yuv420p` report can become `yuva420p` after decoding.

Transparent AVIF is supported when the linked FFmpeg build exposes its color and
auxiliary alpha streams and provides an AV1 software decoder such as `libdav1d`.
The static Dockerfiles enable dav1d; ordinary AV1 decoder selection is unchanged.
The adapter reads AVIF `auxC`/`auxi` properties and `auxl` references to identify
alpha, selects timed tracks instead of animation poster items, and pairs frames
by presentation timestamp. Alpha is full-range coverage. AVIF `prem` references
identify encoded-premultiplied RGB and are handled by the shared source
preparation described below. Logos and manual/playlist layers share this path.
Mismatched color/alpha timestamps, dimensions or missing counterpart frames are
reported as preparation errors; they are not silently rendered opaque. The
current AVIF regression scope covers non-tiled 8-/10-bit stills and three-frame
straight/premultiplied sequences.

VVC alpha remains unsupported by the tested FFmpeg decoder. See the
[format test results](development/graphics-testing.md#alpha-format-checks-2026-11-05)
for exact versions and scope.

## Alpha normalization and source scaling

Preparation accepts straight and encoded-premultiplied sources. Format/container
information, including AVIF `prem`, is authoritative; where the linked FFmpeg
exposes alpha metadata, decoded-frame information takes precedence over stream
metadata because a decoder may already have unassociated its result. AVIF is
special: its separate color decoder has not seen the alpha plane, so the container
association applies to the combined image. Unknown sources retain the legacy
straight-alpha interpretation; there is no pixel-based guessing or UI override.
Older FFmpeg headers without alpha metadata remain buildable.

Unscaled straight sources and opaque sources keep the previous swscale path.
Encoded-premultiplied sources are converted to 16-bit encoded RGB using their
supported matrix/range tags. When resizing an alpha source, straight RGB is first
associated and color/coverage are filtered together. Unassociation happens in RGB
during final quantization to straight YUVA420P. Zero coverage produces zero RGB;
invalid overshoots are clamped. YUV black/chroma offsets are never divided by
alpha. Chroma is formed from coverage-weighted RGB within each 2×2 cell.
Scaled manual/playlist PNGs use this same preparation; unscaled PNG/text keeps its
established conversion. This changes source preparation, not the compositor's
cached resizing between output resolutions or HLS's final rendition scaling.

The current normalization policy supports encoded SDR signals with BT.601,
BT.709, FCC, SMPTE240M or BT.2020-NCL matrix coefficients. RGB and unspecified
matrix tags use the existing BT.601 preparation policy, without inferring a matrix
from resolution. Primaries/transfer tags are retained; gamut conversion, ICC
interpretation, linear-premultiplied RGB, HDR/PQ/HLG and tone mapping are not
implemented by this adapter. Unsupported normalization cases fail explicitly.
The backend-independent `Alpha` descriptor still distinguishes encoded and linear
premultiplication so future GPU backends can retain their native representation.
Decoded-source resources keep their actual matrix/range in frame metadata and use
an unspecified compatibility label, rather than incorrectly claiming BT.601 Limited.
The legacy signal compositor accepts that label for YUV targets without relabeling
or color-managing pixels. Strict matching admission remains unchanged.

Normalization runs on source preparation workers, not on the program video thread
or once per output. Static surfaces are prepared once and cached; animated sources
pay this cost per prepared source frame. Preparation pauses when its bounded
lookahead/cache is full; eviction does not move decoding onto the compositor.

RGB16 association, resizing and YUVA quantization use SIMD. RGBA8 expansion also
uses SIMD; genuine YUV→RGB conversion retains FFmpeg's matrix/range handling.
Large preparation jobs (at least 3.5 million source or target pixels) may share
the same lazy, capped four-thread Rayon executor as composition. One operation is
admitted at a time; other callers immediately use serial SIMD. There is no
per-source or per-output thread pool. Smaller jobs stay serial.

Resize scratch buffers are reused and released when geometry changes or the
preparer is dropped. Additional conversion/scaling workspaces, including a
conservative allowance for cached serial and parallel resize buffers, are bounded
to 256 MiB by a source/target pixel estimate. This is separate from decoder
internals and the prepared-surface cache budget, and is not a process RSS limit.
Threading reduces wall latency, not aggregate CPU proportionally. Large full-frame
animations still need an adequate decoding/preparation budget; see the
[measurements](development/graphics-testing.md#source-alpha-preparation-2026-11-05).

## Quality and animation cache controls

The Graphics page offers **High quality scaling** per manual/playlist layer and
for the Program Logo. It defaults to enabled, including existing definitions that
omit the new `high_quality` field. Enabled uses associated-RGB filtering for alpha
edges; disabled restores faster independent straight-color/alpha scaling and can
show colored fringes. Unscaled compatible images keep their existing fast path.
Encoded-premultiplied sources always receive correct normalization, even with the
switch disabled; this switch does not bypass alpha interpretation or color checks.
In the current implementation premultiplied sources retain the high-quality path
in both modes. Layer quality changes apply on Take/Start or the next clip start;
Apply layout/Show/Hide do not reprepare pixels. The Program Logo updates live.

**Graphics → Settings → Animation cache (MiB)** sets the per-source cache
ceiling for this channel: default 128 MiB, range 32–1024 MiB, allocated on demand.
This affects short-animation caching and byte-bounded streaming lookahead, not
PNG decode limits, output-resolution caches, time/frame limits or decoder buffers.
The shared 256 MiB resource budget remains in force for channel-managed sources;
a larger per-source ceiling does not guarantee a fully cached asset. Multiple
sources and output queues can retain additional memory as described below.

Changing the value does **not** require a playout restart. Newly prepared manual
and playlist graphics snapshot the new limit; existing layers keep their cache
until taken/started again. An enabled Program Logo is reloaded in the background
and keeps its previous surface until the replacement is ready. This avoids
partially discarding a fully cached animation during playback. Channel settings
persist in the database; per-layer quality persists in existing JSON definitions.

## Timing and outputs

A source starts at the first program video timestamp presented to its graphics
stage. File seeks do not seek the logo. Normal clip changes and live takeovers
keep the source and its clock alive. The source position is evaluated from
program presentation time, not elapsed wall-clock time or decode completion.
When the program pauses, the animation pauses too. Hide/fade changes do not reset
its phase. A new source/size or channel restart starts a new animation. Live position and
opacity changes preserve its phase.
Changes to the file's size/modification time are detected at the next clip setup.

One immutable logo surface is selected per outgoing program frame before output
fan-out. Encoding workers cannot independently advance its animation. Matching
composition resolutions share the prepared video; desktop and recording use this
same path. Composition order is video, logo, configured text, then runtime text.
Immutable text surfaces travel with their video frame, preserving scroll/fade
placement through decoding queues and loudness lookahead. HLS still composites at its largest rendition and scales smaller
variants from the result. Source animation does not alter the existing layout
rules of independent output resolutions.

## Preparation and resource limits

Decoding, scaling to YUVA420, alpha bounds and chroma coverage preparation run on
one owned preparation worker per channel source, outside the video thread.
The current single-logo feature does not create workers per output or per clip.
A future multi-layer resource manager can replace this scheduling policy without
changing timing or compositing.

Static images and short animations are fully cached only when they fit all of:

- the configured per-source animation cache (128 MiB by default), including
  cached chroma coverage;
- 256 frames;
- 5 seconds of source duration.

After successful caching, the decoder closes and the worker exits. Source
selection then reuses immutable surfaces without decoding, allocations or a
preparation mutex. Longer sources use up to 500 ms of lookahead, additionally
bounded by frame and byte limits. An idle streaming worker parks until program
time advances or the source is cancelled; it does not poll. Once initial
preparation is complete, an invisible logo also parks its streaming worker.
When shown again it catches up to current program time rather than restarting. Seeking/rewinding and
decoder catch-up run on that worker. Only the color video stream and any associated AVIF alpha stream are decoded; audio packets are discarded.

The Program Logo switches from full caching to bounded streaming if the shared
channel budget cannot admit another cached frame. Evicted frames are removed from
both the private cache and shared view so their leases can be released. If queued
output snapshots or other graphics still hold the budget, it retains at most one
pending decoded frame and retries admission on demand or after a short bounded
wait. It keeps the previous visible surface, remains cancellable and does not
terminate its decoder for transient streaming budget pressure.

The cache budget is not a total process-RSS limit: decoder/scaler storage, the
current decode/preparation frames and snapshots held by existing bounded output
queues also consume memory. Input/prepared dimensions are limited to 8192 pixels
per axis; a prepared frame exceeding the configured cache is rejected with a suggestion to
reduce logo scale. AVIF metadata parsing is bounded to 16 MiB per metadata box
and 4096 entries per collection. Color/alpha pairing retains at most 32 decoded
frames and 256 MiB combined, outside the prepared-surface budget. Both decoders
use one thread and the existing preparation worker; no extra worker is created
for the alpha stream.

The video thread never waits for decoding. Before the first prepared frame, the
logo is hidden. If a requested frame is late, the last valid surface remains
visible. Catch-up selects the image for current program time; obsolete images
are not replayed through the output. Errors are logged once per source revision
and do not terminate playout. Replacing/removing a source or shutting down cancels
and joins its worker; snapshots already in bounded output queues remain immutable
until their consumers release them.

Channels without a logo create no preparation worker. Frames with neither logo
nor text pass through the graphics stage without an additional frame reference.
