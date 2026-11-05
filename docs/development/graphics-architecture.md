# Graphics architecture

## Current boundary

`ff-graphics` is an internal workspace crate. The dependency direction is:

```text
ffplayout application → ff-engine → ff-graphics
```

The application continues to own API, persistence and playlist adapters. The
engine owns media selection, input/network policy, program timing and integration
with the video/output pipeline. Graphics code does not depend on the engine,
application, channels, output protocols, or encoders.

The initial extraction retained the current rendering and blending algorithms.
Follow-up fixes make native text alpha handling explicit and correct, while the
per-frame YUV blend kernel and production compositing policy remain unchanged.
The subsequent animated-logo milestone adds bounded image/video source playback.
That animated-logo milestone did not introduce a general scene controller.
The scene controller now handles native logo/text admission and resource revisions,
while their compatibility adapters retain the established timing and layout rules. No GPU, Servo or linear-light production
compositor is introduced. See [Animated logos](../animated_logos.md).

## Modules and contracts

Implementation files are grouped by responsibility:

```text
crates/graphics/src/
├── scene/                    # layer/revision models, controller, frame snapshots, playlist scheduling
├── resources/
│   ├── surface.rs            # descriptors and bounded backend surface storage
│   └── preparation/          # executor, budget leases, cancellation and result tickets
├── rendering/
│   ├── bitmap.rs             # straight-alpha bitmap preparation
│   ├── blend.rs              # scalar/SIMD plane kernels
│   └── text/                 # native rasterization and bounded glyph-cache eviction
├── timing/                   # presentations, source intervals and text fade/scroll clocks
├── layout/                   # logo geometry and renderer-independent styles
└── ffmpeg/
    ├── compositor/           # YUVA kernels, scene composition and scaling caches
    ├── sources/              # PNG/AVIF/video decoding, alpha preparation and animation caches
    ├── execution.rs          # shared bounded Rayon execution for preparation and blending
    └── text.rs               # native bitmap-to-YUVA preparation
```

`lib.rs` and `ffmpeg/mod.rs` re-export established module paths such as
`ff_graphics::text`, `ff_graphics::scene::SceneController` and
`ff_graphics::ffmpeg::source`. Existing engine, application, benchmark and example
imports remain compatible. This organization does not change algorithms,
threading, pixel ownership or feature requirements. Backend-neutral modules do
not import FFmpeg types. Unit tests stay beside the implementation they verify.

| Module | Responsibility |
| --- | --- |
| `ff_graphics::bitmap` | Straight-alpha RGBA CPU bitmaps and source-over preparation of translucent text/background/subtitle layers |
| `ff_graphics::source` | Backend-independent frame intervals and exact rational loop timing |
| `ff_graphics::ffmpeg::source` | Incremental image/video decoder and reusable YUVA preparation on an owned worker |
| `ff_graphics::animation` | Backend-independent fade/scroll evaluation and named canvas/program-frame preparation context |
| `ff_graphics::style` | Plain logo/text layout, colors, styling and existing transition parameters; no FFmpeg, font-library or GPU handles |
| `ff_graphics::layout` | Backend-independent logo sizing and placement compatibility rules |
| `ff_graphics::text` | Native cosmic-text/Swash shaping and rasterization, cached fonts/glyphs, CPU RGBA bitmap results |
| `ff_graphics::blend` | Existing borrowed-plane scalar/SIMD alpha blend kernels, independent of FFmpeg |
| `ff_graphics::ffmpeg::overlay` | Existing YUVA420 overlays and direct blending into borrowed YUV420 video frames |
| `ff_graphics::ffmpeg::logo` | Decode/scale an already-open input, position/reuse logo frames and composite them |
| `ff_graphics::ffmpeg::text` | Prepare straight-alpha text bitmaps as YUVA420 and apply placement/opacity returned by the common playback evaluator |
| `ff_graphics::ffmpeg::reference_video_frame` | Refcount existing video storage when the pipeline needs another frame handle |

FFmpeg support is an explicit optional `ffmpeg` feature. The crate has no default
features. Native text and plane blending can be built/tested without FFmpeg;
`ff-engine` explicitly enables the FFmpeg backend. Backend frame handles are
confined to that module. Public native text calls use graphics-owned style types,
not cosmic-text types.

`StraightRgbaBitmap` owns tightly packed RGBA8 with unassociated alpha. RGB
remains in the encoded color domain; it does not imply linear-light compositing.
Source-over preparation normalizes RGB by resulting coverage, including when a
translucent text layer is placed over a translucent background. Desktop subtitle
preparation uses the same contract. Glyph coverage is explicitly multiplied by
the requested text-color alpha once because cosmic-text 0.19's Swash pixel path
ignores the base alpha.

The FFmpeg text adapter converts straight RGB to YUV and preserves per-pixel
coverage. Chroma colors are reduced using coverage-weighted RGB so transparent
neighbors cannot dilute edge colors. The video blend kernel applies coverage once.
The signal-domain YUV blend kernels are unchanged. Logo decoding now prepares
immutable timed surfaces rather than reopening and reading a first image per clip.

`TextOverlay::prepare(style, text, TextPreparation { ... })` receives named canvas
size, frame rate and `TextTiming` values. All timing values are canonical program
frame indices, not seconds or raw source PTS ticks. `TextPlayback::evaluate`
accepts separate program/scroll frame indices and returns position and opacity
without any FFmpeg dependency. Backends apply their own pixel alignment rules.

## Compatibility adapters in the engine

`crates/engine/src/graphics/` contains `logo_adapter.rs`, `text_adapter.rs`,
`source.rs` (owned preparation/cache lifecycle), `clip.rs`, `manual.rs`,
`control.rs` and `logo_control.rs` (clip/manual integration and live updates),
`program.rs` (selection before
output fan-out), and `frame.rs` (ready-layer snapshots and composition dispatch).
Rasterization, decoding and blend kernels live in `ff-graphics`.

- `LogoConfig` retains its source path and existing public fields. The adapter
  passes local file inputs and a `LogoStyle` to the graphics backend on its
  preparation worker. The programme graphics source survives clip/live boundaries.
  Selection happens after live timestamp mapping and audio lookahead; fades travel
  with queued video frames. The graphics crate never
  imports the engine's media/network helpers.
- `TextConfig` retains explicit text, filename selection and regex behavior. The
  engine resolves content and passes the resulting string with a `TextStyle`.
  Immutable text snapshots retain placement and opacity with each video frame
  through file/live queues and loudness lookahead. The final compositor blends
  video, logo, clip graphics, manual graphics, configured text, then runtime text; scaled text bitmaps are cached
  per composition resolution.
- Existing exported color/position/weight types are re-exported through the
  engine, preserving callers' import paths.
- Runtime text control, revision handling, live/file transitions and program
  frame indices remain integrated by the existing engine code. Both engine and
  graphics preparation entry points accept the named context.
- The logo adapter exposes only explicit size and opacity accessors; it
  does not dereference into the complete backend API.
- The desktop renderer consumes the same composited program frames as recording
  and encoded outputs. Window-only subtitles/help remain prepared bitmaps; their
  text weight inputs use the graphics-owned enum rather than a cosmic-text enum.

Fan-out retains refcounted video storage rather than copying pixels for each
consumer. A visible logo requires one writable image per composition resolution;
matching consumers share that image. Blending borrows that destination and the
existing prepared overlay pixels. No additional channels are introduced.
Logo repositioning retains refcounted pixel reuse. Configuration-to-style copying
occurs during preparation, not each blend call.

### Cached regions and adaptive CPU execution

Each prepared YUVA surface caches `OverlayBounds`, the nontransparent alpha
extent rounded outwards to whole 2×2 chroma blocks. Pixel storage and original
layout dimensions remain unchanged. Common clipping intersects that extent with
the visible frame region, so logo placement and rendition scaling remain stable.
The scan runs during preparation; replacing/modifying alpha requires new bounds.
Fully transparent or offscreen graphics do no blending work.

The default compositor uses serial SIMD below 3,500,000 visible luma pixels.
Larger regions may use a process-wide Rayon pool capped at four workers (also
limited by available parallelism). Large visible-opacity prepared resources warm
the executor outside frame blending. Small/transparent blend regions do not
initialize it, although large source preparation can use the same pool. A nonblocking permit prevents channels from queueing behind one another:
when another preparation or adaptive blend owns the pool, the caller runs serially. Pool setup
failure is logged once and falls back to serial execution. No pool is created
per channel or frame. The blend path retains borrowed plane views. Source
preparation has separate bounded output-plane views and native SIMD kernels;
see [source preparation and alpha handling](../animated_logos.md).

The threshold is a conservative heuristic derived from local ARM64 measurements,
not a hardware-independent CPU guarantee. At about 3.7M/8.3M pixels, four workers
reduced wall latency by about 3.2×/3.5× at roughly 18%/13% more CPU time. Smaller
regions had proportionally higher scheduling overhead. In the actual 4K scene,
cropping alone reduced both wall and CPU time by about 82%; the adaptive policy
therefore selects serial work for that scene's largest 1.37M-pixel layer.

Explicit `BlendExecution::Serial` and `Parallel` support controlled benchmarks;
both share clipping, opacity rounding and SIMD kernels with the default path.

Composition relative to output scaling remains unchanged. Existing tests cover
shared frames, multiple resolutions, concurrent composition and retained encoder
frames. New layer/layout scopes must preserve these boundaries explicitly.

### Desktop, recording and output fan-out

`ProgramVideo` retains the unmodified program frame and prepares a composited
image per required resolution. Desktop submits through the same `VideoCompositor`
as the parallel encoding groups. The window and its recording retain references
to the same prepared pixels; an encoder consuming that resolution reuses them as
well. A full desktop queue is retried before dispatching to other outputs, so
retrying cannot duplicate their frames. Logo fade opacity belongs to each video
frame, including frames waiting in the desktop queue.

The window no longer loads/converts/uploads a separate program logo or blends
it over the window. Resizing/letterboxing now scales the logo with its video.
Window controls, help and selectable desktop subtitles remain window-only.

HLS keeps its existing behavior: composite once at the largest rendition's
resolution, then scale that image for the smaller variants. Its variant encoder
path is unchanged. Independent outputs at different composition resolutions
still require their own layout; they start from the unmodified program image,
preventing previously composited logos from appearing a second time.

### Common ready-layer snapshots

`ff_graphics::scene::FrameLayers` stores small immutable, already evaluated layer
sets in a fixed array, sorted by `(z, layer ID)`. It requires no per-frame heap
allocation and keeps backend content outside the scene module. It complements
`SceneRevision`: existing text/logo clocks can publish ready layers without
submitting new control commands or allocating command receipts every video frame.

The engine's `GraphicsFrame` maps logo, clip graphics, manual graphics, configured text and runtime
text into this model. The clip scene is a group with its own layer IDs and z-order;
the outer group order deliberately preserves existing configuration/API behavior.
This does not expose a new global z-index setting for legacy logo/text.

`FrameOutput::defer_graphics` replaces the entire snapshot atomically. Parallel,
desktop and recording paths retain that same snapshot with the video, instead of
independently attaching logo, text and clip metadata. Outputs needing immediate
composition use the same dispatch and kernels. A rejected deferred snapshot
therefore cannot leave some layers deferred and others already blended.

`GraphicsCache` belongs to a composition resolution. Logo placement still keeps
destination-pixel sizing and evaluates its existing position expression; text
uses the program reference canvas; clip layers use their declared canvas. Cached
logo placement is invalidated by source, destination or position changes. Ready
surface references, not workers or decoders, travel with frames. Leaving a clip
clears all deferred metadata; already queued frames keep their own resources.
Preparation cancellation/join remains with the existing source/clip owners.

### Native control and preparation state

`graphics/control.rs::ProgramScene` owns a `SceneController::new_local(3, 3)`
for the program logo, configured text and runtime text. Local adapters apply
small ordered command batches through `apply_at_frame`; publication and explicit
acknowledgements happen after the batch. There are no per-frame command queues,
receipt channels or preparation workers. The existing queued API remains available;
queued commands are handled before a local batch, with stale-epoch rejection on
clock rewind. Batches acknowledge commands individually, not transactionally.

Each logo source has a monotonic generation. A replacement reserves a loading
resource; its ready result must match that generation before activation. Failure
marks the pending resource failed while the previous active image remains visible.
Explicit removal clears both the active layer and pending slot. Source owners
cancel and join the previous decoder worker before replacement/removal, so an old
job cannot publish into a new source. Ready results are still generation-checked
at the controller adapter. A source that fails after producing pixels retains its
last-good-frame policy. No crossfade between sources is introduced.

Animated logo frames change ready pixels within one resource generation. Text
placement and opacity can likewise change while the prepared bitmap identity
remains the same. These updates do not create new controller revisions or restart
presentation timing. Existing native fade/scroll evaluation supplies the final
pose; controller presentations use an indefinite, unfaded hold. The evaluated
controller layer controls admission, visibility and ordering of the ready pixels.
Text descriptors name the existing BT.601-limited preparation policy; logo color
metadata retains actual prepared-frame tags without guessing missing values.
Its legacy compatibility label remains unspecified. This does not implement
general color negotiation.

On clip completion, text resources are released but the program logo survives.
Queued snapshots keep their own ready pixels. Clock rewind clears the old epoch
and rebinds current native assets under fresh resource revisions. Each channel
owns its own controller and native preparation owners. Native text rasterization
now runs on the bounded preparation service described below.

### Asynchronous text and channel resource budgets

`ff_graphics::preparation::PreparationService` provides one lazily started CPU
worker per channel, bounded admission, weak completion destinations, cancellation
and explicit shutdown/join. `OutputConfig::graphics_preparation` shares that
service across file/live/fallback decoder instances. The application keeps it on
`GraphicsPreparer` across playout restarts and supplies the same service to dynamic
output updates. Channels without text preparation start no text worker.

Configured and runtime text submit owned settings and preparation context.
The video path polls `OnceLock` results; it does not rasterize, acquire the font
renderer lock or wait for a job. A replacement keeps the last-good text. Removing
text clears immediately and drops its completion ticket. A newer request or ended
decoder invalidates old publication. A completed empty text clears the old image;
a render failure logs once and keeps it. Busy admission is retried at most every
250 ms, with only one desired request retained per producer. A first runtime
request may lazily start the worker; subsequent requests reuse it. Queue/setup
locks use `try_lock`, so contention rejects admission instead of blocking video.

Text fades/scrolling keep the original program-frame context. Late preparation
does not restart timing; a short-lived clip may end before its text is ready.
File/live/fallback transitions own separate completion tickets, while cached ready
frames survive for queued consumers. The previous synchronous preparation API
remains available for tests, examples and non-program uses.

All native adapters share a channel `ResourceBudget`: 256 MiB reserved pixels and
workspace, 1024 resource reservations, and 16 queued/running preparation jobs.
Text raster/convert workspace is reserved before large allocations; PNG decoding
reserves bounded input/pixel workspace. Ready YUVA surfaces and scaled text/scene
surfaces carry leases. Native logo cache frames carry the same leases, shared by
repositioned headers. Queued video keeps those reservations until its last pixel
reference is released. Existing per-source limits (logo cache, playlist PNG and
scene scale caches) remain in force. Reservations may conservatively include
stride/alignment headroom. They do not bound library-internal FFmpeg/font caches,
encoder memory, or total process RSS. Destination text scaling failure skips that
graphic and warns instead of failing the output.

An empty graphics snapshot retires the destination's composition-cache entry
without copying video pixels. Immediate composition clears its local graphics
cache as well. In-flight frames retain their own references, so retiring the last
graphic releases its budget once those consumers finish rather than retaining it
until another graphic appears.

The text worker shares job admission and memory accounting with the long-lived
logo decoder and existing playlist preparation worker; streaming a logo does not
occupy the text executor. These adapters keep their different cancellation needs
and owned handles. Stop cancels queued text, invalidates running publication and
waits outside frame processing for up to two seconds. A timed-out handle remains
tracked; resume cannot create a second worker until it finishes. A resume request
is retained if the old worker is still exiting; subsequent text submissions and
playlist admission retry recovery automatically. Joining the old worker does not
cancel that newer resume intent. An explicitly stopped service stays stopped.
Local font
rendering and PNG reads cannot be forcibly interrupted mid-call; cancellation is
checked before work and before publication. Final service destruction joins its
worker rather than detaching it. Renderer panics are contained by the worker;
failed results do not replace an active surface.

Regression tests compare all four groups against the previous explicit blend
sequence at native and scaled sizes, exercise layout invalidation and queued
resource lifetime, and retain the existing HLS, desktop/recording, animated-logo,
wide scrolling-text and shared-resolution tests.

## Internal scene core

`scene`, `surface` and `playback` now provide a backend-independent first core.
They are usable/tested with `ffmpeg::scene::CpuScene`. Prepared clip scenes now
reach production engine output fan-out alongside the compatibility logo/text
adapters. Application-owned frontend controls, persistence and HTTP endpoints
are documented in [graphics control](../graphics_control.md); they remain outside
the backend-independent scene core.

- `SceneControl::submit` never waits for queue capacity. A full/closed channel
  returns the command to its caller; receipts carry accepted/rejected outcomes
  and the published scene revision. Await receipts outside the video thread.
- `SceneController::advance(program_us)` is the explicit frame boundary. It
  consumes at most one queue-capacity-sized FIFO batch, expires completed layers,
  then publishes one immutable revision before acknowledging commands. It creates
  no workers. Unchanged frames reuse the scene's layer allocation; evaluation
  returns a borrowed iterator and performs no rasterization or file I/O.
- Layers have stable IDs, `(z, ID)` ordering, visibility, base opacity, reference
  canvas pixels and an opaque layout scope. The pipeline remains responsible for
  choosing composition resolution and scaling its result for HLS renditions.
- Prepare requests reserve bounded resource slots. Their revisions are globally
  increasing request IDs **per controller**, even across release/clear/reset.
  Ready/failed results must match the pending key. Take requires ready metadata
  and starts its transition at that boundary; loading time never consumes a fade.
  Starting preparation for a newer revision retains the old active layer.
- A retake of an active layer is rejected; update changes its layout/base opacity
  without resetting its timeline. Clear removes it immediately; cached resources
  may remain. Clear-all also invalidates pending resources. Release removes a
  resource and layers using that revision. Backwards program time clears state
  and rejects the queued old epoch; preparation IDs are not reused.
- `SurfaceStore<T>` owns backend surfaces under count/byte budgets, while scene
  nodes retain only metadata leases. Old queued snapshots keep pixels available;
  retirement runs outside per-frame composition once all leases are released.
  The backend must account actual allocations. This budget excludes the caller's
  decoder/rasterizer scratch space and is not a total-RSS limit.
- The initial surface descriptor declares dimensions, layout, alpha convention,
  color encoding and accounted bytes. It covers current RGBA8/YUVA420 CPU results,
  not a complete generic plane/stride/HDR/capability schema. There is no CPU byte
  accessor on the scene resource; future storage adapters need not read pixels back.
- The CPU scene adapter validates/prepares YUVA420 and reuses the existing text
  RGBA conversion and SIMD compositor. It supports the exact reference canvas
  and explicitly matching color policy; unsupported canvases/colors or absent
  resources skip the affected layer and return counters, without logging per frame
  or failing playout. The scaled path reuses `ffmpeg::surface` (also the text
  adapter's surface/cache implementation) and enforces retained cache budgets.
  The caller supplies an already-writable YUV420 target.

### Repetition and transitions

`Repeat::Count(n)` uses a nonzero **total** play count, including the first play;
`Repeat::Forever` repeats without an iteration counter or accumulated rounding.
Source repetition uses exact rational duration before rounding, including
30000/1001 fps, and declares hide or hold-last on finite completion.

A layer presentation independently supports entrance → hold → exit → repeat pause. A static
surface holds indefinitely by default. Repeating that transition requires a finite
hold and a positive visible duration. `pause_us` defaults to zero and retains an
active transparent presentation between repetitions, so controllers cannot expire
it during the gap. Finite total duration is visible-duration × count + pause ×
(count − 1); no pause follows the last presentation. Stopping in a gap cancels
future repetitions immediately. Durations are integer microseconds on canonical
program time; future configuration adapters translate seconds. Stop fades from the
current opacity, repeated stops do not extend it, and stopping during automatic
exit cannot push its deadline back. Pauses do not advance the clock. Source repeats
and layer repeats are separate: replaying a fade cycle does not implicitly restart
an animated source. Existing logo settings still use their established infinite
source looping; finite repeats are currently an internal capability.

Multi-part lower thirds must be prepared as one graphic before a group fade;
fading overlapping children independently is not equivalent. No general nested
scene renderer is implemented. Translation/scale/easing extensions, shared
asynchronous preparation scheduling, adapter migration and new APIs remain staged
work, rather than hidden requirements of this increment.

### Scene validation and overhead

Tests cover bounded command/resource capacity, rejection of stale completions,
immutable snapshots and delayed retirement, ordered layers, visibility/layout
updates, fades, repeat boundaries, stop/clear and program-clock reset. CPU tests
compare multiple translucent/fading layers byte-for-byte with the existing blend
path and check unsupported color/resource failure containment.

```sh
cargo test -p ff-graphics --all-features
cargo test -p ff-graphics --no-default-features
CARGO_PROFILE_BENCH_LTO=thin cargo bench -p ff-graphics --no-default-features --bench scene_evaluation
```

On the local ARM64 Mac, the optimized metadata-only benchmark measured about
11/24/81/283 ns per frame for 0/3/16/64 layers (median of seven samples of 100,000
frames). It includes state advance plus evaluation, not decoding, rasterization,
resource uploads, blending or output encoding. It is a local overhead measurement,
not a production CPU-saving claim.

### Clip-local event scheduling

`playlist::ClipSchedule` is an internal adapter over the scene core. It accepts
ordered events in integer microseconds relative to an item's **effective playback
start**, after its `in` cut. These are not source-file timestamps or wall time.
Resources must already be prepared by the matching backend; compilation performs
no file access or decoding. It reuses scene-command validation, then stores one
immutable revision per event timestamp. Equal-time actions retain declaration
order. Count/layer/resource/history budgets bound this control-side work; invalid
actions and missing resources return a compilation error with event context.

`frame_at(Some(clip_position_us))` borrows the most recent revision and returns its
evaluation time. Selection performs a binary search without allocation, event
execution, channels, or mutable cursors. Fades/repeats remain anchored to their
original event times: pause, forward seek and backwards seek cannot restart them.
An expired layer ID can be taken again. Clear-all removes only this clip's layers,
keeping prepared resources available for later events. Stop follows the same
idempotent fade-out behavior as interactive scene commands.

`frame_at(None)` suppresses clip graphics during live. Resuming uses the current
scheduled clip position, rather than replaying missed take/stop actions. Positions
before zero or at/after clip duration produce no scene. A skip, shortening or clip
replacement must discard that playback occurrence's schedule, even if an old
position is still available. A repeated playlist item gets a new occurrence.
Compiled revisions retain leases until the schedule is dropped; queued snapshots
may retain them longer, so backend retirement must still respect those leases.

Tests cover seek/pause/live clock semantics, equal-time order, opacity/z updates,
clear/retake, finite/infinite repeats, malformed actions, budgets and lease release.
A CPU test compares scheduled fade pixels against direct blending, including a
backwards seek and unchanged cached surfaces.

The `scene_evaluation` benchmark also measures clip selection plus transition
evaluation, with schedule compilation excluded. A local optimized ARM64 run
measured about 13/72/269 ns per frame for 3/16/64 layers (seven-sample median,
100,000 frames per sample). This measures metadata only, not compositing or total
playout CPU. The empty-schedule case can be heavily optimized by the compiler and
is not a useful predictor of application overhead.

### Engine playback integration

The engine's `ClipPlaybackOptions` accepts optional `graphics::clip::ClipGraphics`.
Construct it from a prepared `CpuScene` and compiled `ClipSchedule` outside the
playback thread, then submit it through `Playout::play_clip` or
`AsyncPlayout::play_clip`. Existing playback methods supply no clip scene.
The application now provides an optional `graphics` playlist field and background
PNG preparation, described below and in [playlist graphics](../playlist_graphics.md).
The typed engine interface itself still performs no asset I/O.

Program graphics select a clip scene from final video PTS minus the occurrence's
initial program PTS. A decoder seek implements the media `in` cut and does not
implicitly move the graphics clock. `ClipGraphics::at_position(item_position_us)`
is the explicit offset for joining a scheduled item after its effective start;
the playlist adapter tracks that offset separately from the source seek.
Seeking within that clock evaluates the original transition, without replaying
events. Each invocation owns an occurrence and discards it on return, including
skip/live-end/restart paths. Queued frame snapshots may outlive that invocation.

Live override marks real live and gap-filled frames as suppressing clip graphics;
actual file video re-enables them. The audio lookahead queue snapshots that flag
with each frame, so delayed file/live frames cannot pick up a later global flag.
Program-wide logos and configured/runtime text keep their existing behavior.
The current draw order is logo → clip scene in `(z, ID)` order → manual scene
in its own `(z, ID)` order → configured text → runtime text.
Full migration of legacy layers into a single z-index domain remains separate.

Shared program frames carry immutable scene/time/asset handles into the existing
resolution compositor. Matching consumers reuse composed pixels; independent
resolutions scale each scene surface with cached placement/opacity. HLS still
composites at the largest rendition and downscales completed video for variants.
Desktop, recording and encoders use the same `ProgramVideo` mechanism. Direct
outputs without deferred-graphics support compose once before encoding.

Exact-reference-canvas blending needs no additional scaling cache. Scaled caches
belong to the prepared clip, not a permanent output: the last queued frame release
drops them together with its assets. Their map lock covers lookup only; different
resolutions have independent cache locks. Limits are eight cached resolutions,
64 MiB retained scaled pixel data per resolution, and 8192 per scaled dimension.
These exclude source assets, FFmpeg/scaler scratch space and queued program frames;
they are not a total-RSS limit. Unsupported/missing surfaces are skipped and
reported at most once per prepared clip, rather than stopping media playback.
The scene-specific dimension limit is applied through the scene scaling cache;
legacy text uses the same scaler with its existing wider scrolling surfaces.

Engine tests compare decoded clips with graphics on/off (source seek, nonzero
program PTS, fixed duration, audio processing enabled/disabled), verify exact
fade pixels/live suppression/backwards seek/next-clip cleanup, queued asset/cache
lifetime, and identical shared-resolution pixel references. Existing HLS, desktop,
recording, live recovery and output-isolation tests remain compatibility checks.

### Application graphics preparation

`player::graphics` owns the JSON event schema and conversion into core events;
application playlist/storage conventions do not enter the graphics crate. `Media`
preserves the optional graphics JSON verbatim, including invalid configuration.
Its JSON is shared through `Arc` so regular media/status snapshots do not copy
the event payload. The background adapter parses/validates it, so a malformed
optional field cannot
invalidate the entire media playlist. Graphics participate in playlist equality,
allowing a graphics-only edit to be saved, while the runtime elapsed-item offset
is skipped during serialization. The frontend keeps this field on save/load.

At item selection, the per-channel preparer submits background work with at most
two active preparation jobs and one waiting occurrence. A new clip cancels obsolete
jobs and replaces the waiting occurrence; a spare worker can prepare the current
clip while old non-interruptible local I/O finishes. Stop cancels jobs and drops
waiting work. Tracked handles and occupied slots survive the shared two-second
join deadline, preventing unbounded overlap across restarts. Clips without
graphics create no preparation job. Completion publishes immutable state through
`OnceLock`; video only polls readiness and evaluates its current clock. Late
readiness never shifts a fade or flashes an already ended graphic.

Paths use channel-storage normalization/containment, including symlink resolution.
PNG preparation is bounded separately from GIF/AVIF/local-video decoding. Short
animations cache timed surfaces; longer sources use tracked background decoders.
Shared source preparation handles alpha metadata, high-quality scaling and
configurable cache limits; see [animated logos](../animated_logos.md). Multiple
takes of matching source/size/quality/playback options reuse resources within an
occurrence. A bad asset omits its dependent actions without suppressing other
layers; malformed event logic omits this clip's scene, never its media. Templates
remain future work; animated playlist assets and manual controls are implemented.

## Audit scope and remaining correctness work

The 2026-11-02 TODO reconciliation treats the existing pipeline investigation as
complete, not the general scene architecture or arbitrary-format color support.
The following current assumptions remain explicit:

- Decoder pixel formats depend on the input; the production graphics compositor
  receives an 8-bit YUV420 program frame and prepared YUVA420 logo/text surfaces.
  Graphic alpha has full luma resolution; cached chroma coverage averages each
  2×2 alpha block. Blending writes directly into a writable/refcounted destination
  selected by the engine, without a full-frame RGB conversion.
- The production plane kernel uses unassociated source code values and effective
  coverage: `round((dst * (255 - alpha) + src * alpha) / 255)`. Graphic opacity
  scales coverage once. This differs in representation from the laboratory's
  premultiplied RGB/YUV paths; their buffers are not interchangeable.
- Native text preparation currently uses fixed limited-range RGB-to-YUV
  coefficients (BT.601-style). Image/video preparation preserves source color tags
  and handles encoded-premultiplied alpha, but production blending still mixes
  encoded signal values. General color conversion between arbitrary surfaces,
  explicit chroma siting and HDR remain separate correctness/format work.
- Current layout/clipping aligns graphic positions/regions to 2×2 chroma blocks.
  Tests cover that existing behaviour; they do not establish exact arbitrary
  odd-pixel placement. Extending chroma reconstruction needs its own reference.
- Existing tests establish scalar/SIMD agreement, straight-alpha bitmap
  source-over, coverage-weighted text chroma and output sharing/layout invariants.
  The independent f64 laboratory references validate their declared color policy;
  an end-to-end production YUV reference with explicit quantization tolerances
  remains separate work.

## Following stages

Native logo/text activation, resource revisions and preparation states now use
the scene controller and share the ready-layer handoff with clip scenes. Existing
text scroll/fade and animated-logo clocks intentionally remain in their
compatibility adapters. Text preparation and shared native asset budgets are now
implemented. Next work can address richer native/playlist layer sources independently.
General rendition-layout and color contracts remain separate work. Keep backend
frame handles and glyph-cache types outside scene nodes.

The minimal core also needs reusable opacity transitions for **static graphics**
such as PNG lower thirds. Existing logo/text fades are the compatibility baseline.
A new layer should enter only after its surface is ready, hold indefinitely or
for an explicit duration, then exit on the program timeline. Separate entrance
and exit durations must work without decoding/rasterizing unchanged pixels again.
A stop during entrance fades from its current opacity; clear removes immediately.
A multi-part lower third must fade as one prepared graphic/group to preserve the
appearance of overlapping children. The timing core and prepared-surface CPU adapter are implemented; production
control integration and general group rendering remain open. The detailed
contract is in Phase 6 of the
[graphics TODO](../../.dev/graphics_todo.md).

A future general surface descriptor must separate pixel layout, color metadata,
alpha convention/premultiplication domain, geometry and storage ownership.
`StraightRgbaBitmap` is one concrete CPU result, not a requirement that every future
renderer expose readable CPU bytes. Renderer/compositor capabilities determine
explicit preparation/conversion paths.

GPU work can replace rasterization and final compositing behind backend adapters.
Do not impose a CPU readback step on every backend or claim zero-copy interop
before measuring the complete video path. Device synchronization and readiness
must have a defined late-frame policy.

A future Servo adapter must receive resolved content/data, layout and scene
revisions and deliver ready surfaces. Its timing capabilities, cancellation,
resource bounds and failure isolation need a separate proposal. Native PTS-based
animation guarantees do not automatically apply to HTML/CSS/JavaScript.

Animated logos belong to time-varying sources, independent of layer transforms
and the compositor. Decode them outside the video path with bounded look-ahead;
select frames by program time and retain the last ready frame when late. Static
logos need no continuously running decoder. See the staged plan in
[graphics TODO](../../.dev/graphics_todo.md).

## Validation

```sh
cargo test -p ff-graphics --all-features
cargo test -p ff-graphics --no-default-features
cargo test -p ff-engine --lib graphics::
cargo test -p ff-engine --lib output::composition
cargo clippy --all-features --all-targets -- --deny warnings
```

Existing SIMD/scalar, alpha and logo layout/cache tests moved with their code.
Additional tests cover translucent straight-alpha composition, coverage-weighted
chroma, actual 50% white text over black, frame-rate-independent animation,
separate fade/scroll clocks and finite/repeating scrolling.
Engine adapter tests check filename/regex selection and pixel-equivalent
rendering against the graphics backend with nondefault styles and program times.
The existing overlay benchmark now calls the graphics crate's actual exported
kernels/backend instead of including engine implementation files:

```sh
CARGO_PROFILE_BENCH_LTO=thin cargo bench -p ff-graphics --features ffmpeg --bench overlay_blend
CARGO_PROFILE_BENCH_LTO=thin cargo bench -p ff-graphics --features ffmpeg --bench yuv_execution
```

The experimental linear-light paths live in the graphics crate's example/benchmark
laboratory; their measurements do not select production rendering policy. Audio
and playout examples/benchmarks remain in `ff-engine`.

### CPU surface and layout contracts

Prepared CPU resources can now carry `SurfaceMetadata`: each plane's dimensions,
byte stride and storage, outward-aligned visible alpha bounds, and actual frame
matrix/primaries/transfer/range/chroma tags. Color identifiers use ISO/IEC 23001-8
values, independently of FFmpeg Rust types. Unspecified tags stay unspecified;
resolution never supplies a guessed color encoding. `SurfaceDescriptor::validate`
checks plane count, dimensions, stride, storage, visible bounds and alpha support.
Opaque future resources may omit CPU metadata; they do not need a byte-access API.

Native RGBA text/PNG conversion records its existing BT.601 limited matrix/range.
It does not invent primaries or a transfer function. Pure YUVA resizing explicitly
copies color tags because `sws_scale` does not copy frame properties. Logo
metadata reflects the prepared frame, including unknown fields; conversion paths
without an explicit color contract must not claim inferred BT.709 or sRGB tags.

Production uses `ColorPolicy::LegacySignal`: existing SIMD/Rayon kernels mix
encoded YUV signal values without additional color conversion. Compatibility
labels and actual frame tags are distinct. `CpuScene::with_color_policy` can opt
into `MatchingSignal`, which requires equal, known supported SDR matrix,
primaries, transfer and range, with centered chroma. Mismatches hide that scene
layer and increment the unsupported report; they never fail media playback.
HDR, arbitrary chroma siting and color conversion remain separate work. Strict
admission is not a new user setting and does not change legacy program graphics.

`LayoutContext` explicitly separates reference-canvas pixels from destination
pixels. Reference geometry stretches by the two canvas ratios, preserving the
existing text/playlist behavior. Program fit/aspect correction precedes graphics.
The YUVA adapter aligns dimensions down to even pixels (minimum two), and aligns
positions down to the even grid after integer scaling; negative coordinates clip
at composition rather than being clamped into view. Arithmetic overflow is
rejected. Native logo placement keeps its existing expression/anchor adapter and
destination-pixel sizing. HLS composes the largest rendition and scales the
completed image for smaller renditions. No new normalized-coordinate, anchor or
fit option is introduced in this increment.


### Hybrid playlist animations

`CpuScene` stores a prepared source containing a static surface, a small immutable
animation, or a streaming source. GIF/video decoding reuses `LogoDecoder`.
Initial preparation runs on the existing application preparation worker. A source
exceeding 256 frames, five seconds or its configured per-source byte ceiling
(default 128 MiB) switches to a background decoder
registered with the channel's `PreparationService`. Scene references to that
service are weak; queued video never becomes the owner responsible for joining
workers. Cancellation tickets only signal/wake. The service retains join handles,
joins outside the video thread and prevents overlapping workers across restarts.

Frame selection uses source timestamps and presentation elapsed time. Source
repeat/completion and fade-cycle repeat remain independent. Each take has a new
anchor; layout/opacity updates and visibility retain it. Exact rational source
periods avoid drift. The streaming decoder coalesces bounded per-presentation
requests and atomically publishes immutable ready surfaces through `arc-swap`.
One worker serves an asset's presentations; seeks happen only there. Obsolete or
inactive demand is abandoned between decoder operations. Short cached sources
have no decoder worker. Inactive or satisfied streaming sources park.

The program thread creates a `FrozenScene` before lookahead or output fan-out.
Its bounded 32-layer container captures the selected surface references and
presentation metadata. Outputs never consult a mutable decoder cursor. The engine
recycles up to 64 unreferenced metadata snapshots to avoid steady-state allocation;
queued snapshots retain their own pixels. This preserves shared-resolution reuse
and largest-rendition HLS composition. Playback-owner slots are counted separately
from queued frames, so the last playback owner cancels decoders even if video is
still draining. Asynchronous publication transfers/adds playback ownership safely.

Cached frames, streaming buffers, scaling caches and queued references carry
channel-budget leases. Streaming lookahead is capped at 16 frames, the configured
per-source byte ceiling and 500 ms per decoder. The global channel pixel/workspace/job budgets still apply;
a temporary budget shortage pauses the decoder, not video. The source's poster
counts towards scene admission, while dynamic pixels count towards the shared
channel budget. These are accounted graphics limits, not process RSS limits.
See `docs/playlist_graphics.md` for finite completion, error behavior and limits.

### Scheduled animation preparation

CPU scene evaluation requests bounded background preparation up to 500 ms before
known takes, visibility entrances and repeating fade entrances. `ClipSchedule`
exposes immutable upcoming boundary states; it does not execute future commands.
`Presentation::next_visible_us` excludes cancelled/finished cycles. Current layers
claim their IDs before future states, so a future retake cannot evict an active
presentation's decoder request. Same-timestamp events use their final snapshot.

A preparation request holds its source-local target fixed until activation. The
worker publishes refcounted lookahead frames (within the existing 16-frame/configured-byte
cache limits), allowing the first visible program tick to choose the correct
source frame without waiting for a decoder round trip. This matters when source
and program frame boundaries differ. Ready windows share accounted pixel leases;
there is no extra decoding thread or pixel copy. Long gaps outside the lead window
still suspend demand and release cache/ready windows. Clip disposal, live suppression
and channel shutdown keep their established cancellation and join boundaries.
Unplanned seeks, late preparation or immediate controls retain last-good/poster
fallbacks; the lead window is a preparation opportunity, not a hard latency bound.

### Source-processing settings

Migration 8 persists `processing.logo_high_quality` (default true) and
`processing.graphics_cache_mib` (32–1024, default 128). Playlist/manual takes have
an independent `high_quality` field, default true. These control source preparation,
not the fast per-frame YUVA blend. The shared channel pixel/workspace budget stays
256 MiB; the per-source cache ceiling is not an RSS reservation or a guarantee of
full caching. New takes/clip preparation snapshot the limit; program-logo changes
reload in the background without restarting playout. Program-logo streaming also
falls back from full caching under shared-budget pressure and retains its last
ready pixels while waiting for leases. See [graphics control](../graphics_control.md).
