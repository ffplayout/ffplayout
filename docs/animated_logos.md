# Animated logos

Logo settings are managed under **Graphics → Program logo** and apply while the
channel is running, including during live ingest. See
[Graphics control](graphics_control.md) for the page, permissions and save behavior.

The existing logo path accepts a static image, animated GIF, or local video file.
Animated sources repeat indefinitely, ignoring a file's finite repeat count. No
new database fields or migrations are required. Existing logo scale, position,
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
- Existing static PNG logos, including a full 3840×2160 graphic.

Other image/video formats depend on the FFmpeg decoders in the build. They are
not all guaranteed animation/alpha formats. An ordinary video without alpha is
an opaque rectangle; its audio is ignored. There is no automatic chroma-keying.
Animated WebP, APNG and image sequences are not part of the tested initial scope.
Use a regular local file, not a network URL, live stream or pipe.

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

- 64 MiB of prepared logo surfaces, including cached chroma coverage;
- 256 frames;
- 5 seconds of source duration.

After successful caching, the decoder closes and the worker exits. Source
selection then reuses immutable surfaces without decoding, allocations or a
preparation mutex. Longer sources use up to 500 ms of lookahead, additionally
bounded by frame and byte limits. An idle streaming worker parks until program
time advances or the source is cancelled; it does not poll. Once initial
preparation is complete, an invisible logo also parks its streaming worker.
When shown again it catches up to current program time rather than restarting. Seeking/rewinding and
decoder catch-up run on that worker. Only the video stream is decoded for playback; audio packets are discarded.

The cache budget is not a total process-RSS limit: decoder/scaler storage, the
current decode/preparation frames and snapshots held by existing bounded output
queues also consume memory. Input/prepared dimensions are limited to 8192 pixels
per axis; a prepared frame exceeding 64 MiB is rejected with a suggestion to
reduce logo scale.

The video thread never waits for decoding. Before the first prepared frame, the
logo is hidden. If a requested frame is late, the last valid surface remains
visible. Catch-up selects the image for current program time; obsolete images
are not replayed through the output. Errors are logged once per source revision
and do not terminate playout. Replacing/removing a source or shutting down cancels
and joins its worker; snapshots already in bounded output queues remain immutable
until their consumers release them.

Channels without a logo create no preparation worker. Frames with neither logo
nor text pass through the graphics stage without an additional frame reference.
