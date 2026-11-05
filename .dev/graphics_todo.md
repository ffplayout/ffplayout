# Agent TODO: Native Graphics Engine and High-Quality Compositing for ffplayout

## Objective

Design and incrementally implement a native, CPU-first graphics subsystem for ffplayout that supports layered broadcast graphics, deterministic animations, reusable templates, and high-quality alpha compositing.

The existing YUV-based video pipeline must remain the primary processing path. Avoid unnecessary full-frame RGB conversions.

The architecture must support both an optimized YUV compositing path and an optional high-quality linear RGB compositing path. Linear-light blending is a quality policy, not an assumed CPU optimization. Renderer selection, compositing policy, and storage location are separate decisions.

**Do not introduce Chromium, CEF, Electron, Node.js, or another browser runtime in the current implementation.** Define an adapter contract for a potential future Servo integration, but do not implement or require Servo. GPU rendering and GPU compositing must also remain optional future backends.

## General Instructions

- Inspect the current repository before making architectural decisions.
- Identify the existing YUV/alpha compositing implementation, logo renderer, text overlay renderer, frame representation, and processing pipeline.
- Reuse existing functionality wherever possible.
- Preserve current APIs, configuration behavior, and output compatibility.
- Avoid additional frame copies, allocations, or color conversions in the hot path.
- Keep the graphics engine independent of encoders, output destinations, and playlist scheduling.
- Follow existing Rust conventions and project architecture.
- Implement the work incrementally, with tests and benchmarks.
- Do not perform a large rewrite unless clearly justified.
- Keep backend-specific FFmpeg, cosmic-text/Swash, and future GPU/Servo types out of the scene and control models.
- Keep API, persistence, and playlist adapters in their existing architectural layers; they translate into graphics commands rather than becoming engine dependencies.

---

## Graphics frontend and manual control — 2026-11-04

The shared Graphics page now provides manual graphics, existing Text controls and
Program logo tabs. General Settings links to the logo tab; logo fields/database
and the existing renderer/scaling behavior are preserved. Manual graphics use a
separate bounded program group (eight layers), shared immutable output snapshots,
channel preparation budgets and off-video-thread source preparation. HTTP commands
require observed revisions; stop/clear invalidates obsolete completions. Placement
preview and program-pixel dragging are available in both the playlist and manual
editors. The browser preview does not simulate final video/color/timing. See
[Graphics control](../docs/graphics_control.md) for limits and recovery semantics.
Manual layer settings and requested playback/visibility state now persist per channel,
with DaisyUI collapse editors and background restoration. Reusable templates and
exact render-backed previews remain open. The shared placement canvas supports
checkerboard transparency, program-pixel dragging, proportional corner resizing,
fit-to-canvas and center snapping with alignment guides. Tiny graphics use an
expanded dashed drag area with only the bottom-right resize handle; numeric sizing remains
available. It edits source size;
applying a new size to a running manual layer still requires Take/Start.
Status-poll recovery clears retrieval errors without hiding command failures.
A fresh playout resets the manual graphics clock with its new program timeline;
Clear within the same run preserves the current clock.
Historical milestone descriptions below predate this additional HTTP/frontend work.

Program-logo changes now apply through a lazy, tracked preparation/retirement
worker without restarting the channel. Position/opacity reuse source pixels and
preserve animation phase; source/scale changes swap only after readiness. Pending
updates are bounded/latest-only and reset safely across preparation-service
restarts. Relative output placement, clip/ad fades and HLS rendition behavior are
preserved. Invalid enabled paths/layouts are rejected before saving from the logo
page; failed asynchronous replacements retain the active source.

## Implemented Groundwork — Workspace Crate Extraction

- [x] Introduce an internal `ff-graphics` workspace crate with one-way engine → graphics dependencies.
- [x] Move native text rendering and existing blend/layout code while preserving algorithms and public engine configuration behavior.
- [x] Keep native text bitmap/style APIs independent of FFmpeg and font-library types; isolate FFmpeg surfaces and compositing in an optional backend module.
- [x] Keep media path/regex selection, network-opening policy and pipeline/program-time integration in engine adapters.
- [x] Reuse refcounted logo pixels without additional per-frame channels, workers or frame copies.
- [x] Define straight-alpha CPU bitmap results, correct text/background/desktop alpha preparation, and apply requested glyph opacity once.
- [x] Extract existing text fade/scroll evaluation into a backend-independent component with explicit program-frame timing and named preparation context.
- [x] Keep logo backend access behind explicit engine-adapter methods rather than `Deref`.
- [x] Route desktop program graphics through the shared video compositor; reuse prepared pixels for desktop, recording and matching encoder resolutions. Preserve largest-rendition composition and subsequent downscaling for HLS variants.
- [x] Cache outward-aligned alpha bounds during preparation and skip transparent borders without changing original layout dimensions or copying frames.
- [x] Unify serial/parallel clipping and SIMD execution; use a shared, capped Rayon pool only for large visible regions and serial fallback when busy.
- [x] Measure both wall and aggregate CPU time, including a self-contained area sweep and byte-identical full-scene comparison. Keep small cropped overlays serial.
- [x] Update the overlay benchmark to use graphics-crate kernels/backend and document the extraction in `docs/development/graphics-architecture.md`.

Extraction is complete. The first internal scene core now provides bounded
commands, immutable revisions, ready-resource leases and program-time fades/repeats.
Prepared clip scenes now run through the engine's shared output compositor via
typed engine playback options. Ready logo/text/clip snapshots now share one
ordered layer handoff and composition dispatch. Native logo/text activation,
replacement and resource states now use `SceneController`. CPU plane/color descriptors and bounded native preparation are implemented;
general backend negotiation and new asset renderers remain separate work.
GPU, Servo and templates are not implemented. Manual HTTP/frontend controls are
implemented in the application adapter; see Phase 8.

---

## Status reconciliation — 2026-11-04

Checkboxes below distinguish the existing CPU logo/text implementation from the
planned general scene engine. A completed item may explicitly name its current
scope; that does not imply support for arbitrary layers, formats or backends.
Sources of evidence:

- [Graphics architecture](../docs/development/graphics-architecture.md): crate
  boundaries, program-time evaluation, shared composition, CPU execution and tests.
- [Animated logos](../docs/animated_logos.md): supported source formats, looping,
  cache/lookahead bounds, failure policy and worker lifetime.
- [Blend experiments](../docs/frame-blend.md): independent color-policy references
  and benchmarks; experimental linear-light paths are not production features.
- [Playlist graphics](../docs/playlist_graphics.md): implemented JSON operations,
  source/transition timing, preparation limits and failure policies.
- [Graphics testing](../docs/development/graphics-testing.md): regression coverage,
  release load measurements, entrance preparation and remaining long-run limits.
- Implementation/tests in `crates/graphics/src/`, `crates/engine/src/graphics/`,
  and `crates/engine/src/output/composition.rs`.

**Current milestone:** the first minimal scene/layer core is implemented internally
and tested through the CPU compositor. Native logo/text resource state now runs
through the common controller; their established scroll/fade and source-frame
clocks remain the compatibility baseline.
The engine accepts prepared clip scenes, evaluates their clip-local clock and
passes immutable snapshots through output fan-out. Existing logo/text ordering
and HLS rendition/layout behaviour remain the compatibility baseline. The optional
playlist JSON adapter now prepares static PNGs and bounded GIF/AVIF/video assets asynchronously. Templates,
SVG, GPU and Servo remain separate work. Manual HTTP controls and the shared
Graphics page are implemented (Phase 8). Do not repeat the
completed extraction or scheduling/engine integration. Recent increments add bounded entrance preparation, first-visible-frame
selection from ready lookahead, metadata-aware alpha preparation, per-source
quality controls and a configurable animation cache (migration 8).

**How to read this status:** checked items describe the implemented CPU/native
scope only. Partially delivered requirements are split into completed foundations
and unchecked extensions. Compatibility pixel comparisons and short smoke runs
are not independent color-quality validation or a 24/7 stability certification.
The detailed phases below are reconciled with the delivered milestones above.

Completed-scope evidence map:

| Area | Implementation and regression evidence |
| --- | --- |
| Commands, layers, revisions, resource state | `scene/controller.rs`, `scene/playlist.rs`; ordered updates, stale completions, clock reset and bounded admission tests |
| Worker and pixel lifetime | `resources/preparation/`, `resources/surface.rs`; shutdown/restart, leases, cancellation and native resource-budget tests |
| CPU layout, metadata and composition | `layout/context.rs`, `ffmpeg/compositor/`; checked geometry, tag preservation, cache/pixel compatibility and unsupported-layer tests |
| Cached/streamed assets and preparation | `ffmpeg/sources/animation.rs`, `ffmpeg/sources/animation/streaming.rs`, `timing/`; repeats, seeks, entrance phase, idle decoding and 20-cycle cleanup tests |
| Production integration | engine `graphics/` and app `player/graphics.rs`; shared output handoff, pre-first-take preparation, occurrence disposal and playlist JSON tests |
| Measured load | `benches/animated_scene.rs` and [Graphics testing](../docs/development/graphics-testing.md); 0/3/8 layers, 1080p/4K25/50, Rust allocation counters and short soak runs |

Graphics-only implementation paths above are relative to `crates/graphics/src/`
unless qualified as an engine/application/benchmark path.

**Next work, in priority order:**

1. Extend the completed prepared-YUVA signal reference through RGBA preparation,
   color conversion and independent visual fixtures (Phases 3/4/10).
2. Full-channel long-run, simultaneous multi-channel and output fan-out measurements
   with animated graphics, live ingest and repeated replacements (Phase 10).
3. Generalized anchors, normalized/relative placement and explicit fit/aspect
   policies (Phase 2). The implemented browser Fit button is not this layout model.
4. Exact renderer-backed preview and reusable templates with editable text/image
   groups (Phases 7/8); the current preview is a browser placement aid.
5. SVG/shapes and slide/scale/easing/crossfade transitions as independent features
   (Phases 5/6), followed by broader source/backend contracts.

GPU/Servo/HDR and production linear-light compositing remain optional later work.
The unchecked items below retain their detailed acceptance requirements; ordinary
manual controls, persistence and placement editing are no longer pending work.

### Scene core delivered in this increment

- [x] Stable layer IDs, explicit reference-canvas pixel positions/scopes, deterministic
  `(z, layer ID)` order, visibility and opacity updates.
- [x] Bounded FIFO control with nonblocking submission and explicit per-command
  acknowledgements/rejections after publication at a frame boundary. No command
  coalescing: prepare/ready/take/stop/clear ordering is retained.
- [x] Immutable scene revisions and ready-resource leases; old frames retain their
  own resource references. Unchanged frames share the existing scene allocation.
- [x] Separate source repeat and layer-transition repeat policies. Counts include
  the first play; `Forever` means unbounded repeats. Finite source completion
  explicitly selects hide or hold-last. Existing animated-logo configuration still
  loops forever; finite repeats are an internal capability, not a new UI setting.
- [x] Static layer entrance/hold/exit, separate durations, indefinite hold, finite
  or infinite fade cycles, stop from current opacity, and immediate clear.
- [x] Increasing preparation revisions reject stale results after replacement,
  release or clear-all. A program-clock rewind clears the current epoch and
  rejects queued old-epoch commands. No new decoding/preparation workers are added.
- [x] CPU resource storage bounded by surface count and accounted bytes, with
  lease-based retirement. Unsupported/missing scene surfaces are skipped without
  failing video. Existing CPU YUV kernels and text bitmap conversion are reused.
- [x] Regression tests for ordered layers, stop/clear, late results, resource limits,
  immutable snapshots, differing FPS, long-running/finite repeats and pixel equality
  with existing blending. A separate `scene_evaluation` benchmark measures only
  metadata advance/evaluation, not rasterization or full-frame performance.

### Common program-frame handoff delivered

- [x] Map ready logo, clip-scene groups, configured text and runtime text to
  backend-independent ordered `FrameLayers`; retain stable group IDs and the
  existing logo → clip graphics → configured text → runtime text order.
- [x] Replace independent deferred logo/text/clip setters with one atomic
  `GraphicsFrame` snapshot, retained by parallel, desktop and recording outputs.
- [x] Use the same composition dispatch for immediate and deferred outputs;
  preserve destination-pixel logo placement, reference-canvas text scaling and
  largest-rendition HLS composition. No new workers, pixel copies or per-frame
  allocations for the layer container.
- [x] Clear deferred metadata on occurrence completion while queued frames retain
  ready resources; test four-group pixel compatibility, layout invalidation,
  layer ordering and cache/resource lifetime.

### Native controller integration delivered

- [x] Track program logo, configured text and runtime text as three bounded
  controller layers/resources. Translate native controls at frame boundaries;
  publish controller-approved layers through the existing shared snapshots.
- [x] Provide single-owner frame-boundary batches without control queues or
  receipt allocation. Keep the queued control API for other adapters and explicitly
  retain its ordering, acknowledgements and stale-epoch behavior.
- [x] Tag logo producer results by source generation. Track loading/active/failed
  replacements, keep good pixels while replacements load/fail, and reject retired
  results after replacement/clear. Expose old active resource status independently
  of its newer loading/failed replacement.
- [x] Keep decoding/rasterization with existing native source/decoder owners;
  cancellation/join still happens at their lifetime boundary. Frame snapshots
  retain only ready pixels. No new preparation workers or per-frame decoding,
  rasterization, pixel copies or resource-generation changes are introduced.
- [x] Preserve animated-logo source clocks and legacy text scroll/fade evaluation;
  avoid applying a second controller fade. Release text on occurrence completion,
  preserve the program logo across clips/live, and rebind resources on clock rewind.
- [x] Test batched publication, loading/failed replacement, stale results, queued
  resource lifetime, stop/clear, resource reuse across animation/scroll frames,
  channel isolation, backwards clocks and native/scaled pixel compatibility.

### Asynchronous preparation and shared resource budgets delivered

- [x] Prepare configured and runtime native text on one lazy, persistent CPU
  preparation worker per channel. Frame processing polls ready cells, retains
  last-good text during replacement, and never waits for rasterization.
- [x] Keep only the latest text request per producer; drop/clear/replacement
  invalidates old completion tickets. Retry a busy queue at most every 250 ms;
  late results evaluate the original program fade/scroll phase.
- [x] Share a 256 MiB pixel/workspace reservation budget, 1024 resource leases
  and 16 queued/running job permits across native text, logo preparation and
  application playlist graphics. Preserve tighter source-specific limits.
- [x] Carry leases with cached source/scaled pixels, including queued frame
  references. Logo repositioning shares its lease. Last-reference destruction
  releases budget without a retirement queue or per-frame pixel copies.
- [x] Reserve text/PNG workspace before large allocations. Bound text input and
  canvas size; skip a failed destination graphic instead of stopping media.
- [x] Cancel queued text work and invalidate running publication on shutdown;
  join outside frame processing with a deadline, retain unfinished worker handles,
  and prohibit overlapping replacements across restarts. Logo streaming and
  playlist preparation retain their existing owned, cancellable worker lifetimes.
- [x] Test stale/cleared text, failures, overload/retry, worker reuse, panic
  containment, shutdown deadlines, restart exclusion, shared leases and pixel
  equality with the established synchronous text path.

### CPU surface/layout contracts — 2026-11-03

- [x] Describe prepared CPU plane dimensions, byte strides/storage, visible alpha
  bounds and actual matrix/primaries/transfer/range/chroma tags. Validate their
  consistency; preserve unknown tags explicitly and avoid guessing by resolution.
- [x] Tag native text/PNG matrix/range and preserve metadata through pure YUVA
  resizing. Logo descriptors read prepared-frame metadata.
- [x] Preserve fast SIMD/Rayon YUVA signal blending as the production compatibility
  policy. Add optional strict SDR/color/siting admission for CPU scenes; unsupported
  layers are reported/skipped without failing video. No new UI setting.
- [x] Introduce backend-neutral reference/destination pixel layout contexts, checked
  scaling arithmetic and explicit YUVA alignment/clipping rules. Use the context
  for shared text/scene scaling; preserve logo placement and HLS rendition behavior.
- [x] Test padded planes, metadata validation/preservation, unsupported color and
  alpha combinations, differing aspect ratios and negative/overflowing coordinates.

**Still open in this milestone:** generalized anchors/normalized coordinates,
renderer capability negotiation and conversion planning beyond current CPU SDR,
and adapters for future independent scene sources. The shared budget bounds
accounted graphics pixels/workspace and jobs, not FFmpeg/font-library internals
or total process RSS. Group fades operate on one prepared surface; no nested-scene
renderer or source crossfade is implied.

---

### Hybrid animated playlist sources — 2026-11-03

- [x] Support GIF and local video playlist assets through the existing logo decoder
  and common CPU scene compositor, with source timestamps independent of playout FPS.
- [x] Cache sources within 256 frames/five seconds/configurable bytes (128 MiB default) completely; switch larger
  assets to tracked background decoding with up to 500 ms/16 frames/configurable byte lookahead.
  Restrict inputs to regular local files and non-nesting demuxers; check cancellation.
- [x] Coalesce up to 32 presentation requests per streaming asset, support worker-side
  seek/looping, discard inactive/obsolete work and park satisfied/hidden sources.
- [x] Freeze selected surfaces before audio lookahead/output fan-out; recycle bounded
  frame metadata and retain queued pixels independently from playback owners.
- [x] Cancel on final playback-owner disposal; join media workers through the channel
  service off the video thread, with tracked handles and restart exclusion.
- [x] Separate `source_repeat`/`source_end` from fade repeats; allow finite/forever,
  hide/hold-last, deterministic seek/live return and a fresh clock on every take.
- [x] Account cached/poster pixels through scene admission and all dynamic pixels through shared channel leases;
  snapshots keep caches alive until the final consumer releases them.
- [x] Test GIF timing, multiple playout FPS, retakes, finite completion, local video,
  cancellation, partial budget failure, scaled composition and playlist JSON.

- [x] Compare overlapping variable-delay animation/fade cycles at 24/25/30/50/60 FPS
  and repeat streamed take/stop/clear/live-suppression lifecycles with complete
  reservation release. Add a paced release load/soak benchmark with zero-layer
  baseline, 1080p/4K canvases, CPU/RSS, latency and Rust allocation measurements.
  See [Graphics testing](../docs/development/graphics-testing.md). Extended 24/7
  full-channel stability validation remains open.

- [x] Prewarm planned takes/visibility entrances and fade-repeat entrances within
  500 ms, select the first visible program tick from ready bounded lookahead,
  retain program-time phase across differing FPS, and park longer invisible gaps.
  Keep current presentations ahead of future retakes and preserve queued pixels.
  Cover pre-first-take engine integration, source-frame boundary crossings,
  finite repeats, stop/clear, resource retirement and zero-graphics overhead.

Animated PNGs and further decoder/source unification remain separate work.
Single frames exceeding the configured cache still require smaller dimensions. Program-logo streaming uses the same configurable per-source ceiling and falls
back to bounded decoding under shared channel-budget pressure. Evicted private
and published cache references release their leases; temporary pressure retains
the last ready image while the worker waits. See [source processing settings](../docs/graphics_control.md#source-processing-and-animation-cache).

---

## Phase 1 — Investigate the Existing Pipeline

**Priority: Critical**

- [x] Locate all existing graphics-related code.
- [x] Identify current logo and text-overlay rendering paths.
- [x] Identify the exact pixel formats used during decoding, processing, compositing, and encoding.
- [x] Determine whether the current YUV+alpha implementation uses straight or premultiplied alpha.
- [x] Verify how alpha blending is calculated for Y, U, and V planes.
- [x] Check how chroma subsampling and chroma location are handled.
- [x] Identify whether current compositing operates directly on video frame planes or intermediate buffers.
- [x] Review frame ownership, synchronization, memory reuse, and thread boundaries.
- [x] Verify whether graphics are applied before output rendition scaling.
- [x] Identify existing SIMD optimizations and available CPU features.
- [x] Document bottlenecks and correctness concerns.

### Deliverable

Completed: [graphics architecture](../docs/development/graphics-architecture.md)
documents the current paths, ownership, integration boundaries and following stages.
The audit records existing assumptions; it does **not** certify arbitrary
colorimetry, chroma siting, odd-pixel placement or HDR. Those correctness and
format-extension tasks remain open in Phases 3 and 4.

Do not replace existing functionality during this phase.

### Existing Investigation and Benchmark Inputs

Reuse and review the work already present rather than starting from scratch:

- `crates/graphics/src/`: native text rendering, plain style types, shared plane kernels, and the optional FFmpeg CPU compositor backend.
- `crates/engine/src/graphics/`: compatibility adapters for media-derived text and source-opening policy.
- `crates/graphics/examples/frame_blend.rs` and its supporting modules: color models, independent references, linear-light SIMD and text-rendering experiments.
- `crates/graphics/benches/overlay_blend.rs`: single-plane microbenchmarks and complete-overlay comparisons, including direct SIMD calls and one-/four-worker pools.
- `docs/frame-blend.md`: assumptions, timing boundaries, measurements, and limitations.

These experiments are evidence and reusable test material, not production architecture. Existing measurements favor the current YUV compositor for speed. Do not infer aggregate CPU savings from lower wall time with multiple workers; caller/worker scheduling and CPU core placement also affect comparisons.

---

## Phase 2 — Define the Graphics Architecture

**Priority: Critical**

Introduce a graphics subsystem with clearly separated responsibilities.

Suggested responsibilities (not mandatory public types or separate crates):

```text
API / configuration / playlist adapters
                 ↓ commands
Graphics state + resource preparation
                 ↓ immutable scene revision
Canonical program time → scene evaluation + layout context
                 ↓ evaluated layers / render requests
Native renderer / future Servo adapter
                 ↓ prepared surface references
CPU compositor / future GPU compositor
                 ↓ program frame
```

- The scene contains backend-independent content descriptions, resource identifiers, transforms, stacking order, and animation definitions.
- Scene evaluation determines presentation properties for a program timestamp; it does not perform file I/O or rasterization.
- Renderers prepare graphics surfaces; compositors combine ready surfaces with the video frame.
- The resource manager owns caches, preparation jobs, budgets, cancellation, and retirement of unused surfaces.
- Backend-specific adapters own frame/texture handles and synchronization. Never require a scene node to contain an FFmpeg frame, glyph cache, or GPU texture.

### Tasks

- [x] Define graphics layer identifiers and deterministic z-ordering.
- [x] Support layer visibility, position, opacity and static reference-canvas scaling
  through `LayerSpec`, updates and the CPU layout adapter.
- [ ] Add general layer transforms beyond current static scaling/placement.
- [x] Separate scene state/evaluation from rendering; native logo/text and prepared clip graphics use the shared production handoff.
- [x] Separate logo/text preparation from final video compositing; native layers use the controller and immutable scene/resource state.
- [x] Introduce a bounded, nonblocking internal command interface with explicit acknowledgements/rejections and overload behavior.
- [x] Preserve FIFO command ordering without coalescing in the initial core; reject saturation explicitly.
- [x] Publish immutable scene revisions at an explicit caller-supplied program-frame boundary.
- [x] Admit bounded preparation requests with acknowledgements and loading/ready/active/failed resource states; require ready resources before activation.
- [x] Guard current native/playlist preparation with resource revisions, source generations, scene epochs and weak publication tickets; reject retired completions.
- [x] Cancel and join current text, logo, playlist-preparation and streamed-asset workers outside frame processing; retain unfinished handles across restart.
- [x] Keep current file preparation/decoding/rasterization off the video thread and consume ready results. Templates remain separate; manual HTTP controls are implemented in the application adapter.
- [x] Support cached static/animated logo sources and prepared text, with shared native preparation admission/budgets. Future renderer scheduling remains separate.
- [x] Define native resource ownership, shared leases, cancellation and worker lifetime; future backend contracts remain separate.
- [x] Contain recoverable native preparation/decode/composition errors by skipping a failed layer or retaining last-good pixels; malformed playlist graphics do not stop media.
- [x] Require ready surfaces before take; pending/failed preparation cannot replace an active surface. Missing/unsupported backend resources hide that layer. Keep existing animated-source last-good-frame policy.
- [x] Consume ready CPU surfaces without waiting for unfinished preparation/decode jobs. Future backend synchronization guarantees remain part of Phase 11.
- [x] Integrate bounded ready-surface storage, shared pixel leases and worker handoff with production logo/text/clip graphics; queued frames retain their own references.

Suggested commands:

```rust
enum GraphicsCommand {
    Load { /* ... */ },
    Play { /* ... */ },
    Update { /* ... */ },
    Stop { /* ... */ },
    Clear { /* ... */ },
    ClearAll,
}
```

Preserve compatibility with existing logo and text APIs by mapping them to the new internal model.

### Layout and Rendition Contract

- [x] Define reference/destination pixel canvases, checked scaling, even alignment,
  negative-position clipping and existing program-fit-before-graphics behavior.
- [ ] Add generalized anchors and configurable graphic fit/aspect rules.
- [x] Distinguish reference-canvas and destination-pixel sizing explicitly through
  `LayoutContext`/`PixelSizing`; preserve legacy logo sizing.
- [ ] Add normalized/relative coordinates with explicit conversion semantics.
- [x] Compose existing logo/text once at the largest HLS rendition and scale smaller renditions from it. Preserve this in the future shared-program layout contract.
- [x] Carry explicit `Scope` and concrete layout contexts for CPU scene selection.
- [ ] Expose selection/configuration of arbitrary destination-specific scene scopes
  through application controls; legacy logo placement remains a compatibility adapter.
- [x] Preserve existing logo/text sizing and placement semantics through compatibility adapters; make any behavior change explicit and tested.
- [x] Keep `LayoutContext` and `Scope` independent of encoders/protocols; engine/output adapters supply canvas dimensions and select composition.
- [x] Test mixed output resolutions/aspect ratios, shared pixels, CPU scene scaling/clipping/overflow and largest-HLS-rendition composition without duplicate logos. Anchor/relative-coordinate tests follow those features.

---

## Phase 3 — Pixel Formats and Color Management

**Priority: Critical**

Do not force all graphics into RGBA8 or YUVA420.

Create an abstraction that supports different graphics surfaces and compositing formats.

### Tasks

- [x] Preserve YUV as the primary program video format.
- [x] Compare YUVA420/YUVA444 in the blend laboratory. Production remains YUVA420; general capability/format negotiation remains open.
- [x] Support full-resolution alpha in existing straight RGBA bitmaps and prepared YUVA420 surfaces; chroma uses separately prepared coverage.
- [ ] Avoid irreversible chroma subsampling during initial graphics rendering whenever practical.
- [x] Define explicit pixel-format and CPU plane-layout metadata; opaque backend storage remains optional.
- [ ] Extend composition beyond the documented CPU 4:2:0 alignment/coverage contract to arbitrary chroma siting; descriptor coverage is already implemented.
- [x] Describe actual CPU primaries/transfer/matrix/range/chroma metadata, retain
  unknown values explicitly and preserve tags through pure YUVA resizing.
- [ ] Validate and implement general color conversions beyond the current SDR
  compatibility/strict-admission policies; preserving tags alone is not conversion.
- [ ] Design for future 10-bit and HDR support without requiring immediate implementation.
- [x] Distinguish straight, encoded-premultiplied and linear-premultiplied alpha; the CPU YUVA path accepts straight alpha only.
- [x] Carry explicit AVIF `prem` and available FFmpeg frame/stream alpha metadata
  through source preparation. Unknown sources keep the legacy straight interpretation;
  do not guess from pixels. Preserve builds using older FFmpeg headers.
- [x] Normalize encoded-premultiplied source RGB with 16-bit intermediates and
  zero-alpha/overshoot handling before straight-YUVA composition. Keep encoded/linear
  alpha descriptors independent of future backend storage.
- [x] Scale new alpha source surfaces in associated RGB, fuse unassociation with
  final YUVA quantization and prepare coverage-weighted 2×2 chroma. Reuse this for
  scaled manual/playlist PNGs and keep unscaled compatible sources on the old path.
- [x] Test real straight/premultiplied 8-/10-bit AVIFs and animations, soft/hidden
  edges, tiny alpha, YUV offsets and native-path compatibility. Add reproducible
  1080p/4K wall/CPU/RSS source-preparation measurements in graphics-testing.md.
- [x] Expose per-layer/Program Logo high-quality scaling (default on), retaining
  premultiplied-alpha normalization in both modes. Include quality in asset reuse keys.
- [x] Persist a per-channel animation-cache setting (32–1024 MiB, default 128 MiB).
  Apply it without playout restart to new preparations and reload an enabled
  Program Logo; existing layers adopt it on their next Take/Start. Keep shared
  resource, frame/time and output-cache limits independent.
- [x] Optimize source alpha preparation with runtime SIMD, packed ARM64 NEON
  quantization, exact RGBA8 expansion and SIMD associated-RGB16 filtering.
  Share the capped four-thread graphics executor with immediate serial fallback;
  reuse geometry-bounded scratch rather than creating per-source pools.
- [x] Verify serial/parallel band boundaries, scalar/SIMD quantization (within one
  output code value), exact alpha association, immutable input and scratch reuse.
  Compare reference/serial/four-thread/adaptive preparation for RGB16, RGBA8 and
  YUVA8 with separate wall/aggregate CPU/RSS measurements.
- [ ] Extend alpha-aware filtering to the existing cached resizing of already
  prepared YUVA between output composition resolutions; source preparation above
  does not change HLS rendition behavior.
- [ ] Add an explicit color policy for linear-premultiplied/HDR source normalization;
  the current SDR adapter rejects these cases instead of treating them as encoded RGB.
- [ ] Ensure alpha is not incorrectly applied to offset-encoded YUV components.
- [x] Describe current CPU surfaces with independent format, color, alpha, geometry and storage metadata.
- [x] Specify encoded versus linear premultiplication separately from pixel layout (`Alpha`); current CPU YUVA preparation requires straight alpha.
- [x] Describe current full-resolution alpha, visible bounds, plane dimensions/strides and actual chroma siting; arbitrary-siting composition remains open.
- [x] Implement backend-owned/refcounted CPU surfaces and generic `SurfaceStore<T>` leases without byte access in scene/resource control types.
- [ ] Leave storage extensible to opaque backend resource references with explicit lifetime and readiness contracts. Do not implement placeholder GPU resource types yet.

Suggested abstraction:

```rust
enum GraphicsPixelFormat {
    Yuva420,
    Yuva444,
    Rgba8,
    Rgba16F,
}
```

Treat these as candidate formats, not mandatory implementations. Adapt them to the actual frame types already used by ffplayout. This enum describes pixel layout only; storage residency, color information, and alpha conventions are separate descriptors.

### Backend Capabilities and Conversion Policy

- [ ] Define renderer output capabilities and compositor input capabilities without requiring every backend to implement every format.
- [x] Prepare/cache current PNG, text and short animations outside frame processing;
  stream longer assets on tracked workers and reuse scaled surfaces where possible.
- [ ] Generalize conversion planning to additional renderer/compositor capabilities
  and measure new unavoidable conversions separately.
- [x] Validate CPU descriptors/alpha/layout and skip unsupported composition; document
  legacy signal blending versus optional matching-known-SDR admission.
- [ ] Extend rejection/fallback negotiation to future formats and backends.
- [x] Preserve fast signal-domain YUVA in production and independently measured
  linear-light RGB references in the blend laboratory; do not conflate their semantics.
- [ ] Decide whether to expose a production linear-light policy after separate
  quality/performance evaluation; it is not currently selectable.
- [ ] Allow a future GPU backend to own final compositing as well as graphic rasterization.
- [x] Keep CPU byte access and full-frame RGB conversion out of backend-neutral
  scene/control contracts. Current implementation requires no GPU upload/readback.
- [ ] Evaluate device sharing, synchronization and transfers before implementing
  a GPU renderer/compositor contract (Phase 11).

### Acceptance Criteria

- No mandatory RGB conversion of the full program frame.
- No avoidable quality degradation during graphics preparation.
- Pixel-format and color metadata are explicit.
- Future renderers can provide graphics without changing the engine's control API.

---

## Phase 4 — Correct Alpha Compositing

**Priority: Critical**

Implement and validate a mathematically consistent alpha-compositing model.

### Fast Path — YUV Compositor

Continue using the existing YUV compositing approach where appropriate.

- [x] Verify straight-alpha source-over onto opaque video against an independent
  f64 signal-domain oracle, including every input byte pair and quantization bounds.
- [x] Support partial transparency and global opacity in the current CPU YUV compositor.
- [x] Preserve transparent/opaque alpha endpoints; skip zero-alpha pixels and cached transparent borders, retaining SIMD handling for visible regions.
- [x] Verify offset-preserving blending for matching BT.709 limited/full-range
  source/target samples, exact endpoints and unchanged metadata.
- [ ] Validate source preparation and range/matrix conversion for mismatched inputs.
- [ ] Handle chroma subsampling without introducing unnecessary color bleeding.
- [x] Apply documented even alignment, visible-region clipping and alpha bounds
  for current CPU YUV420 composition, including negative/odd requested coordinates.
- [ ] Validate chroma quality independently and decide whether exact odd/subpixel
  placement warrants a new composition path.
- [x] Prepare and reuse 2×2-averaged coverage for the existing 4:2:0 U/V planes. Arbitrary chroma siting remains open.
- [x] Preserve existing performance optimizations.

Pay special attention to chroma blending at partially transparent edges.

### Correctness Reference — Signal-Domain YUV

- [x] Add a separate f64 signal-domain reference with explicit BT.709 limited/full
  signal offsets and 2×2 alpha coverage averaged before byte quantization.
- [x] Compare SIMD dispatch and serial/parallel/adaptive frame blending against
  that reference with documented rounding bounds; keep f64 between reference layers.
- [x] Test exact transparent/opaque endpoints, single-sample chroma coverage,
  saturated red/green over blue, opacity gradients, overlapping layers, even
  negative/edge clipping, SIMD tails, strides and unchanged padding.
- [ ] Extend independent references through RGBA preparation, nonuniform source
  chroma within a 2×2 cell, mismatched color metadata and arbitrary siting. These
  tests currently begin with prepared straight-alpha YUVA420P samples.

### Quality Reference — Linear RGB Compositor

Create an independent reference implementation for validating compositing quality.

- [x] Convert test inputs into a defined linear-light RGB working space.
- [x] Support mathematically correct source-over operations with premultiplied alpha.
- [x] Convert the resulting image back to the required YUV format.
- [x] Use sufficient numerical precision for reference calculations.
- [x] Compare YUV compositing with this reference for documented visual differences; do not require pixel equality between different compositing policies.
- [x] Document intentional differences between signal-domain and linear-light blending.

Completed in the `frame_blend` laboratory and documented in
[Blend experiments](../docs/frame-blend.md). These checks cover its explicit SDR
matrix/range/transfer and chroma assumptions, not arbitrary production metadata.
The reference implementation does not run in the production video hot path.

### Acceptance Criteria

- No unexpected halos around transparent edges.
- No incorrect darkening from mismatched alpha conventions.
- No obvious chroma artifacts caused by incorrect alpha sampling.
- Correct behavior when multiple semitransparent layers overlap.
- Documented and reproducible differences between fast and reference compositing modes.

---

## Phase 5 — Native Graphics Rendering

**Priority: High**

Implement native rendering without browser dependencies.

Initially support:

- [x] PNG graphics with transparency through the existing logo source/preparation path.
- [ ] SVG graphics.
- [x] Native shaped/rasterized text with existing style and playback adapters.
- [ ] Basic shapes and colored rectangles.
- [x] Independent layer opacity with take/update and multiplicative transition opacity; no rerasterization for opacity-only changes.
- [x] Existing logo/text positioning and scaling through compatibility adapters. General layer transforms remain open.

Evaluate lightweight Rust rendering libraries such as `tiny-skia`, `resvg`, and the existing ffplayout text-rendering implementation.

Do not replace existing dependencies without benchmarking or establishing a concrete functional need.

### Animated Logos and Time-Varying Graphics Sources

Animated logos are an explicit native graphics requirement. Keep the existing
logo configuration/control path as the compatible user-facing entry point, but
implement animated content as a reusable graphics source within a layer, not as
animation-specific logic inside the logo compositor.

```text
Logo configuration adapter → Graphics layer
                              ├─ static or animated graphics source
                              └─ layer transforms / opacity / transitions
                                           ↓ ready surface
                                      Compositor
```

- [x] Separate source-content animation (changing image frames) from layer animation (position, scale, opacity, and transitions). Both must be usable together.
- [x] Select ready cached/streamed CPU assets by source-local time derived from the
  canonical program clock (`PreparedSurface`); freeze pixels before output fan-out.
- [ ] Unify remaining legacy-logo and playlist-source internals behind a reusable
  renderer/source adapter contract beyond the current FFmpeg CPU implementation.
- [x] Document/test the initial PNG, variable-duration GIF, alpha qtrle video and opaque FFV1 scope. APNG, animated WebP and image sequences remain outside the tested scope; see `docs/animated_logos.md`.
- [x] Preserve source frame durations, including variable durations, and map them to program presentation time rather than advancing one animation frame per video frame.
- [x] Define internal finite/forever source timing and hide/hold-last completion,
  independently from layer fade cycles. Existing animated-logo user settings
  retain their previous infinite-loop behaviour.
- [x] Integrate playlist `source_repeat`/`source_end` with cached and streamed assets;
  a clear/take restarts source phase, while fades/visibility preserve the source clock.
- [ ] Expose independent source restart and finite-repeat settings through future
  program-logo UI/API controls; existing logo settings still loop forever.
- [x] Define behavior across pause, seek, clip boundaries, live takeovers, and timestamp resets using the canonical timeline. Decoder completion time must not become the animation clock.
- [x] Decode and prepare animated content outside the video processing thread with bounded look-ahead in time, frame count, and decoded bytes.
- [x] Avoid fully preloading arbitrarily long animations; evaluate bounded reuse for short loops separately.
- [x] When a frame is late, retain the last valid surface without blocking playout; before the first valid surface, keep the layer hidden. Select the frame appropriate to current program time when rendering catches up rather than replaying an obsolete backlog.
- [x] Distinguish delayed decoding from malformed/unsupported assets; apply the layer failure policy and expose useful status without repeated per-frame error logs.
- [x] Cancel obsolete jobs and release decoded buffers/decoder resources on replacement, stop, clear, and shutdown. Guard results with source revisions.
- [x] Static logos must not require a continuously running decoder or an additional worker. Share bounded preparation resources where practical rather than creating a thread per layer.
- [x] Keep the compositor unaware of whether a surface came from a static logo, animated source, native renderer, or future backend.

Implemented initial scope: existing logo configuration maps to a persistent
program source with local PNG/GIF/video decoding, exact rational loop periods,
configurable-byte/256-frame/5-second caching, bounded streaming lookahead, immutable per-frame
selection before fan-out, error containment and cancellation/join. See
`docs/animated_logos.md` for supported test formats and limitations. The current
single-logo implementation uses one owned worker per channel source. Playlist
assets additionally support finite/forever source playback, retakes, bounded
multi-presentation requests, scheduled fades and 500 ms entrance preparation.
Internal graphics commands are implemented; general HTTP/frontend graphics controls
and a unified adapter contract beyond current CPU sources remain open. Cached
static assets retain no decoder or active decoding work during playback; the
shared preparation service itself is persistent and parks when idle.

This source abstraction should also support future animated lower thirds and
other graphics. It must not depend on Servo or GPU rendering, and it must not
force future sources to return CPU-readable byte buffers.

### Rendering and Caching

- [x] Reuse current native text/PNG/logo pixels across frames; reprepare on relevant content/layout changes rather than on opacity-only updates.
- [x] Cache decoded static images and short logo animations within documented bounds.
- [ ] Cache parsed SVGs when an SVG renderer is introduced.
- [x] Reuse the native font/glyph renderer and prepared text/scaled surfaces. A general bounded text-layout/resource cache remains open.
- [x] Reuse immutable static logo/text surfaces and bounded animated source frames across program frames.
- [x] Reuse unchanged prepared layer surfaces and immutable scene revisions; changing
  one layer does not rerasterize all other native layers.
- [ ] Evaluate dirty-region/offscreen scene rendering when a renderer that needs it
  is added; fresh program video frames still require composition. Keep any dirty
  regions bounded and useful for that renderer rather than for video-frame reuse.
- [x] Reuse immutable pixel references, alpha bounds and shared-resolution output
  composition; first-visible-frame lookahead adds references, not pixel copies.
- [ ] Profile remaining native preparation/conversion buffers before introducing
  further clear/copy optimizations; no universal minimum-copy claim is established.
- [x] Distinguish rasterization invalidation from video compositing: a cached static surface must still be composited onto every fresh video frame.
- [x] Key current asset reuse by path/size/source-playback options, native resources
  by revision/generation and scaled caches by selected surface/geometry. Reuse
  glyph rasters through the native font cache.
- [ ] Define a general renderer cache key including template/font/style/color/alpha
  and backend/device identity when those additional resource types are introduced.
- [x] Bound current CPU pixel/workspace reservations, resource leases, jobs, scene layers/history, asset dimensions and scaled caches; native-library internals/process RSS are outside those bounds.
- [x] Retire current CPU resources via shared leases, weak completion tickets, revisions and tracked cancellation; final playback-owner disposal cancels streamed work.
- [x] Test CPU cache eviction/replacement, stale publication, queued-frame ownership,
  shutdown deadlines and worker restart exclusion/resource release.
- [ ] Specify backend-specific retirement/fence behavior for future GPU/Servo adapters.

---

## Phase 6 — Deterministic Animation System

**Priority: High**

Animations must use program presentation timestamps rather than wall-clock timing.

Implement:

- [x] Implement backend-independent fades for prepared static/animated scene
  surfaces; production logo/text use the common controller/handoff while preserving
  their established fade/scroll adapters rather than applying a second fade.
- [ ] Translation and slide animations.
- [ ] Scale animation.
- [x] Configurable fade-in/hold/fade-out and repeat-pause durations through internal transitions and optional playlist JSON; future slide/scale durations follow those features.
- [x] Evaluate current fades with linear easing on program time.
- [ ] Add selectable nonlinear easing functions for supported transitions.
- [x] Complete finite fade cycles and source repeats on program time, hide or hold the final source frame explicitly, and bound all clip graphics by occurrence lifetime.
- [x] Stop from current opacity, clear immediately and replace explicitly via
  clear/take; reject accidental retakes of an already active layer.
- [ ] Add smooth resource-replacement/crossfade transitions as a separate feature.

Example principle:

```rust
animation_time = current_program_pts - animation_start_pts;
```

### Static graphics and lower-third transition contract

- [x] Support entrance → hold → exit independently from source duration; static
  pixels can hold indefinitely and finite/forever fade cycles can include pauses.
- [x] Internal transitions use separate fade-in/out durations in integer microseconds
  on program time, with linear easing. The playlist adapter translates user-facing
  seconds for playlist and manual HTTP controls. Zero duration gives an immediate take/stop.
- [x] Support indefinite hold until `Stop`, or finite hold followed automatically
  by fade-out, all on program time. Fade cycles may repeat a finite number of
  times or forever only when hold is finite and total cycle duration is positive.
- [x] Require ready pixels for activation. Playlist fades keep their scheduled
  program-time anchor: late preparation joins the current phase and expired events
  do not flash or shift the playlist. Prewarm planned entrances when possible;
  preparation latency does not restart the source or fade.
- [x] Evaluate effective opacity as source alpha × configured layer opacity ×
  transition opacity, applying each factor once. Cache static pixels; changing
  opacity must not trigger rerasterization, decoding or a full-frame RGB conversion.
- [x] Apply one transition to a lower third already flattened into a prepared
  surface, including all its alpha; this satisfies the initial single-surface scope.
- [ ] Define isolation/composition for editable background/logo/text groups or
  nested scenes. Independently fading overlapping children is not group opacity.
- [x] `Stop` starts fade-out from the currently visible transition opacity, even
  during fade-in. A repeated stop must not restart/extend the exit. Retire the
  active presentation at fade completion; reusable cached resources may remain
  within budget. `Clear` removes that layer immediately; shared preparation/cache
  resources may remain ready but cannot activate it automatically. `ClearAll`
  invalidates pending preparations as well.
- [x] The scene-core controller rejects taking an already-active layer; playlist
  replacement follows its clear/take contract. Manual Take explicitly replaces
  and restarts its single layer, while Start reuses unchanged prepared resources.
  Layout/opacity updates preserve the clock; smooth crossfading remains separate.
- [x] Test clear/take source restarts, obsolete-request retirement, cancellation
  and queued-pixel lifetime for current cached/streamed CPU assets.
- [x] Implement manual Stop/Start and explicit Take restart, retaining prepared
  resources when unchanged; Show/Hide preserves animation phase. Updated control
  fades apply to subsequent Show/Hide and Stop without altering an ongoing fade.
- [ ] Add source-only restart independent of the layer transition and smooth
  replacement/crossfade transitions with cancellation.
- [x] Keep transition evaluation independent of rasterizer, decoder and compositor
  backend, so future GPU/Servo surfaces can use the same native opacity timeline.
- [x] Test current fades with zero/short durations, differing program FPS, base
  opacity, stop during entrance/pause/exit, repeated stop/retake, stale preparation,
  clip/live suppression, shared HLS scaling and cached-pixel reuse. See playback,
  controller, compositor, engine graphics and application playlist-graphics tests.
- [ ] Extend these checks to slide/scale/easing, editable groups and new backends
  when implemented.

### Requirements

- Animations must behave consistently at different frame rates.
- Pauses, seeking, clip transitions, and live-source changes must have documented behavior.
- Do not allow cumulative animation timing drift.
- Ensure consistent results across HLS, SRT, and other output paths.
- Define behavior for discontinuous or reset timestamps.

Use a canonical program timeline rather than assuming that source PTS values are continuous.

Native animations must be evaluated from that time, independent of rasterization completion or worker scheduling. A future external/HTML renderer must declare its timing capabilities separately; do not promise deterministic CSS/JavaScript animation merely because the native timeline is deterministic.

---

## Phase 7 — Graphics Templates

**Priority: Medium**

Introduce a declarative JSON-based graphics template format.

Templates should support:

- [ ] Versioned template schema and explicit supported features.
- [ ] Canvas dimensions, coordinate units, anchors, and scaling policy.
- [ ] Multiple layers.
- [ ] Images, text, and shapes.
- [ ] Dynamic text properties.
- [ ] Positioning and opacity.
- [ ] Basic animation definitions, including entrance/exit fades and indefinite or finite hold for static graphics/lower thirds.
- [ ] Runtime data updates.
- [ ] Animated source references and explicit playback/repeat settings, separate
  from layer animation definitions. Expose once/finite-total-count/forever
  explicitly; source loops and repeating fade/slide cycles are independent.
  Static content normally holds until stopped, rather than "looping" identical pixels.

Planned template example, not implemented or accepted as a playlist graphic.
The current experimental event schema is documented in
[playlist graphics](../docs/playlist_graphics.md):

```json
{
  "name": "lower-third",
  "canvas": {
    "width": 1920,
    "height": 1080
  },
  "layers": [
    {
      "type": "rectangle",
      "x": 80,
      "y": 840,
      "width": 900,
      "height": 160
    },
    {
      "type": "text",
      "value": "{{name}}",
      "x": 120,
      "y": 875
    }
  ]
}
```

Template parsing and resource loading must occur outside the real-time video processing path.

Validate template data and resource paths before activation.

---

## Phase 8 — Control API and Integration

**Priority: Medium**

Expose the graphics subsystem through the existing ffplayout API architecture.

Potential operations:

```text
GET    /api/graphics/{channel}
POST   /api/graphics/{channel}/take
PUT    /api/graphics/{channel}/{layer}
DELETE /api/graphics/{channel}/{layer}
POST   /api/graphics/{channel}/clear
```

Adapt paths and naming to existing project conventions.

### Tasks

- [x] Map existing native logo control to controller layers; preserve existing fade semantics.
- [x] Map existing configured/runtime text overlays to controller layers; preserve their clocks.
- [x] Keep existing logo/runtime-text API entry points compatible; current new clip graphics use optional playlist JSON rather than replacing endpoints.
- [x] Expose internal resource/controller state for current loading/active/failed revisions.
- [x] Expose manual program-graphics state through authenticated HTTP and frontend inspection; legacy logo/text retain their existing configuration interfaces.
- [x] Apply internal/playlist layer updates and take/stop fades at frame boundaries without restarting playout.
- [x] Add a reusable frontend playlist-event form with local file selection,
  action ordering, sparse updates, fades, repeats and independent source playback.
  Preserve existing JSON/unknown fields and cancel edits without mutating the playlist.
- [x] Add authenticated manual take/update/stop/visibility/clear controls on a
  shared Graphics/Text/Program-logo page, with program-pixel placement preview
  reused by the playlist editor. Keep legacy logo sizing and HLS behavior.
- [x] Add shared checkerboard placement preview, proportional resize handles at
  top-left/bottom-right, fit-to-canvas and center snapping with guides. Publish
  combined size/position edits atomically. Check all four locale key sets.
- [ ] Add exact renderer-backed canvas/video preview.
- [ ] Add reusable graphic templates (Phase 7).
- [x] Define internal FIFO submission and equal-time playlist declaration order without coalescing.
- [x] Require observed revisions for manual HTTP commands; reject intervening changes/preparation completions with conflicts.
- [x] Define current worker cancellation/join/restart exclusion, clock-epoch reset and clip-occurrence disposal.
- [x] Persist manual layer settings/names and requested running/visibility state per channel; release runtime resources at stop and restore visible active layers in the background. Hidden/stopped layers prepare lazily; fade/source phase is not persisted. Templates remain separate.
- [x] Keep current controllers, preparation services, budgets and playlist file containment per channel; controller isolation is tested.
- [x] Test manual graphics authorization/channel isolation, revision conflicts, failed sources, and scoped administrator-only logo saves through HTTP routes.

Do not introduce breaking changes to existing clients.

---

## Phase 9 — Playlist-Based Graphics Events

**Priority: Medium**

Design optional timeline events that allow graphics to be activated or stopped during playlist playback.

Example:

```json
{
  "graphics": [
    {
      "at": 2.0,
      "action": "take",
      "layer": 100,
      "source": "graphics/lower-third.png",
      "fade_in": 0.4,
      "fade_out": 0.4
    },
    {
      "at": 10.0,
      "action": "stop",
      "layer": 100
    }
  ]
}
```

### Requirements

- [x] Execute prepared clip graphics against the engine's program timeline through
  typed playback options; application playlist JSON remains separate.
- [x] Define internal event timing relative to an item's effective playback start,
  after its `in` cut, in integer microseconds (`playlist::ClipSchedule`).
- [x] Bind engine scene lifetime to the playback occurrence so shortened,
  skipped or interrupted clips cannot keep activating layers. Application
  playlist navigation must still supply the correct occurrence/position.
- [x] The internal adapter compiles bounded immutable boundary snapshots once;
  per-frame selection never re-executes events or restarts fades after seeking.
- [x] Define internal live/jump behavior: suppress clip graphics during live;
  resume at the current scheduled clip position, with no catch-up event playback.
- [x] Keep existing playlists valid without modification; preserve optional
  graphics data through save/load and compare it when detecting playlist changes.

The adapter takes already prepared resource leases and supports take, update
(including independent opacity and z-index), stop, visibility and clear. It
preserves declaration order for equal timestamps, handles finite/infinite
transitions, and bounds event count, concurrent layers, resources and retained
scene-history nodes. Out-of-order/out-of-range events and invalid actions fail
compilation before rendering. Empty schedules produce no graphics.

### Engine integration

- [x] Typed `ClipPlaybackOptions`/`ClipGraphics` on synchronous and asynchronous
  engine playback; legacy methods default to no additional graphics.
- [x] Evaluate clip-relative time independently of the media `in` cut; support an
  explicit item-position offset when joining the schedule part-way through.
- [x] Suppress clip graphics on live/gap video, and retain that decision with each
  frame through audio lookahead. Leave the established program logo/text active.
- [x] Drop each occurrence on clip return/skip/restart; queued immutable frames
  retain prepared surfaces and scaling caches until their final consumer finishes.
- [x] Share composition for matching output resolutions, including desktop and
  recording, and retain largest-rendition composition/downscaling for HLS.
- [x] Reuse the text surface scaler for scene layers. Bound additional caches to
  eight resolutions, 64 MiB retained scaled pixels per resolution and 8192 pixels
  per scaled dimension. Unsupported/missing graphics skip without aborting video;
  warn once per prepared clip rather than once per frame.
- [x] Test decoded media with `in`/duration, nonzero program PTS and audio processing
  on/off, plus live suppression, seeking, cache lifetime and shared pixels.

### Application integration

- [x] Optional per-item `graphics` events: PNG take/retake, layout/opacity/z update,
  stop, visibility, clear and clear-all. Separate finite/forever transition cycles.
- [x] Optional invisible `pause` between transition cycles, with no trailing pause
  after the final repeat; stop during a gap cancels future repetitions.
- [x] Prepare local graphics off the video thread with at most two active background
  jobs and one waiting occurrence per channel. Publish via a nonblocking readiness cell and weak cancellation
  ticket; late results use the current scheduled phase rather than restarting it.
- [x] Keep preparation state/slots on the channel across restarts, cancel publication
  on stop and join tracked work with a bounded wait. Retain occupied slots/handles after
  timeout until the old job finishes. Keep scene-only limits out of legacy text.
- [x] Preserve effective-item elapsed time independently from source `in` during
  startup/live resume, including still images and live transport playlist items.
- [x] Enforce channel storage containment, symlink checks, file/decoded-size bounds,
  source-surface budgets and per-clip reuse of matching path/size combinations.
- [x] Omit failed sources without disabling unrelated layers. Malformed event
  configuration omits graphics, preserves its JSON and leaves media playing.
- [x] Document usage and limits as experimental in `docs/playlist_graphics.md`.

This adapter supports static PNGs plus cached/streamed GIF/AVIF/video assets. The
frontend playlist editor now provides a reusable action form, validation and
read-only JSON inspection. Automatic splitting of graphics-bearing clips is
explicitly disabled until per-part timelines can be reconstructed. Shared canvas
placement editing and manual live HTTP controls are implemented (Phase 8).
Templates, exact renderer-backed preview, animated PNGs and migration of all
legacy layers into one z-index domain remain separate work. Existing animated
program logos remain supported. The playlist adapter itself needs no schema
change; manual graphics persistence extends migration 8 and uses GET/POST
`/api/graphics/{channel}`.

---

## Phase 10 — Testing and Performance

**Priority: Critical**

### Visual Correctness

Create reproducible reference test cases for:

- [x] Verify white antialiased text over black with the native renderer; the glyph alpha is applied once.
- [ ] Saturated red text over blue video.
- [x] Numerically validate semitransparent alpha gradients on prepared saturated
  YUVA surfaces; independent visual/rasterization fixtures remain separate.
- [ ] Soft shadows.
- [x] Compare overlapping CPU scene layers/fades with existing surface blending for pixel compatibility.
- [x] Add independent f64 numerical references for overlapping prepared transparent graphics.
- [ ] Add independent golden-image references including asset rasterization and preparation.
- [x] Exercise transparent PNG borders and cached alpha bounds in the existing compositor; broader color-policy references remain open.
- [x] Test documented even-alignment/clipping of odd/negative CPU graphic positions.
- [ ] Independently validate subpixel/odd-position chroma quality; arbitrary-siting support remains open.
- [x] Validate matching BT.709 limited/full signal blending numerically.
- [ ] Validate BT.709 conversion/preparation end to end, including chroma-edge quality.
- [x] Test multiple staggered layers with animated opacity against independently selected source frames and existing blend kernels.
- [ ] Add independent color/alpha quality references for multilayer animated-opacity scenes.
- [ ] Color-conversion round trips.

Validate each optimized compositor against an independent reference with the same mathematical policy. Separately compare signal-domain YUV and linear-light RGB visually and numerically, without treating their intentional differences as implementation errors.

Use numerical comparisons where appropriate and retain visual regression test images.

### Performance

Benchmark:

- [x] Compare CPU YUV kernels with experimental RGBA/linear variants in `overlay_blend`/`frame_blend`; benchmark scene metadata in `scene_evaluation` and actual cached/streamed CPU scene composition in `animated_scene`.
- [x] Benchmark existing static logo/text composition, including shared-resolution reuse.
- [x] Benchmark cached GIF/streamed video layers with fade/pause cycles in `animated_scene`; fixtures are 320×180 graphics, not rendered editable templates.
- [x] Benchmark 0/3/8 overlapping layers with a no-graphics baseline in fresh processes.
- [ ] YUVA420 versus YUVA444 graphics preparation.
- [x] Benchmark full-frame work versus cached alpha bounds/ROI clipping.
- [x] Run short 1080p25/50 and 4K25/50 scene benchmarks; these use small overlays on the stated canvases, not full-frame 4K animations.
- [x] Count Rust allocations/reallocations and allocated traffic across benchmark threads after warm-up.
- [ ] Measure native-library allocation traffic and large-asset decode/preparation separately.
- [x] Measure aggregate process CPU and peak RSS alongside sampled graphics reservations.
- [ ] Measure memory bandwidth; current allocated-byte counters are not bandwidth measurements.
- [x] Run three-minute 4K/50 graphics-only smoke checks with worker join and zero reservations after cleanup.
- [ ] Validate 24/7 full-channel operation with encoding, HLS renditions, live ingest and output failures.
- [x] Direct caller-thread SIMD versus persistent worker pools, with wall latency and aggregate CPU measured separately.
- [x] Unit-test idle preparation services, updates/clear/replacement, eviction,
  failed/cancelled jobs, restart exclusion and bounded CPU reservations.
- [ ] Run simultaneous multi-channel/idle-channel load and repeated configuration
  reload tests through complete playout pipelines.
- [x] Test late/out-of-order current CPU preparation results, stale revisions and
  failed/missing sources without resurrecting retired graphics or stopping media.
- [ ] Extend failure-containment tests to future renderers/backend synchronization;
  in-process tests do not establish containment of native process crashes.
- [x] Test shared-resolution reuse, independent output composition, scene scaling/cache lifetime and largest-HLS-rendition composition.
- [ ] Benchmark full scene fan-out across simultaneous outputs in an end-to-end run.
- [x] Test logo/source timing across program FPS, variable GIF delays and rational loop periods; also test scene fade repeats/pauses and first-visible-frame selection across a source-frame boundary. Slide/scale/easing remain open.
- [x] Cover current cached/streamed CPU sources with seek/retake, malformed/cancelled
  preparation, bounded requests, stop/clear, long-gap suspension and planned entrance
  preparation. Queued frames retain frozen pixels; obsolete requests are retired.
- [ ] Expand malformed/slow-codec asset coverage and run sustained replacement/
  overload fault injection beyond current deterministic fixtures.
- [x] Verify bounded current cache/request storage, cached-source pixel reuse,
  parked satisfied/inactive decoders and complete retirement across 20 source lifecycles.
- [ ] Run multi-hour animated program-logo and playlist-source loops, including
  large assets and allocator/RSS trends; a short smoke test is insufficient.

### Performance Goals

- No per-frame allocations in the steady-state hot path where avoidable.
- No unnecessary full-frame conversions.
- No blocking operations on the video processing thread.
- No regression for channels without active graphics.
- Predictable resource usage during continuous 24/7 playout.

Do not claim performance improvements without measurements. Establish a baseline before implementation and repeat focused benchmarks at each relevant stage, rather than postponing performance validation to the end.

---

## Phase 11 — Future Extensions (Do Not Implement Yet)

Keep the architecture extensible for:

- Servo-based HTML/CSS graphics rendering.
- Optional GPU acceleration.
- HDR graphics and wide-gamut color.
- Higher bit-depth compositing.
- Animated vector graphics.
- Advanced transitions and graphical effects.
- External fill/key outputs.
- Preview/program graphics workflows.

### Future Adapter Contracts to Document Now

Current CPU ownership, timestamps, cancellation and resource budgets are already
documented in the architecture report. The unchecked items below concern the
additional negotiation, interop, timing and isolation rules for future adapters;
they do not reopen the implemented CPU scene/control contract.

- [ ] Document how a renderer receives content/data updates, layout changes, scene revisions, and render timestamps or deadlines.
- [ ] Document delivery of ready surfaces, alpha/color descriptors, storage ownership, and last-good-surface behavior.
- [ ] Document capability reporting, unsupported combinations, and timing guarantees per backend.
- [ ] Document cancellation, resource budgets, shutdown, and failure containment requirements for a future Servo adapter.
- [ ] Require explicit evaluation of stronger failure isolation before adding an external/HTML renderer; a scene abstraction alone cannot contain a process crash.
- [ ] Keep any future Servo-specific HTML/CSS/JavaScript behavior behind its adapter rather than adding browser concepts to native scene nodes or control commands.
- [ ] For GPU integration, evaluate renderer-to-compositor resource sharing, device identity, synchronization, and transfers across the video pipeline before choosing an implementation.

Do not implement Servo, GPU backends, or speculative plugin-loading infrastructure now. Document these contracts and implement only the CPU interfaces needed by existing logo/text functionality. Do not add unnecessary abstractions solely for hypothetical future features.

---

## Implementation Order

The phases above describe work areas, not one large delivery. Use these independently reviewable milestones:

1. **Audit and baseline:** document existing behavior, reuse current experiments, and establish matched-policy correctness tests and performance baselines. Fix demonstrated YUV issues separately.
2. **Minimal modular core:** define scene revisions, surface descriptors, layout scopes, bounded commands, and resource ownership. Adapt existing logo/text paths and CPU YUV compositing while preserving behavior. Include reusable, program-time opacity transitions for static graphics/lower thirds (take, hold, stop); keep existing logo/text fades compatible. General slide/scale/easing extensions follow later. No template system, new public API, or new renderer is required to complete this milestone.
3. **Resource lifecycle:** add asynchronous preparation/activation where needed, stale-result rejection, bounded caching, cancellation, and failure policies. Validate hot-path allocation behavior and 24/7 resource stability.
4. **Native rendering extensions:** add animated logo sources as a separate, reviewable feature using the shared source/surface contract and bounded decoding. Add shapes/SVG and richer text independently as needed. Benchmark preparation/decoding separately from per-frame compositing.
5. **Animation:** extend the canonical program timeline and layout rules with deterministic native transitions.
6. **Templates:** introduce the versioned declarative schema and validation as an independent feature.
7. **Control integration:** expose additional operations through existing application API/configuration architecture while preserving current clients.
8. **Playlist events:** add optional scheduling adapters with explicit seek, skip, interruption, and live-takeover semantics.
9. **Optional quality/backend work:** decide on production linear-light compositing, Servo, or GPU implementations in separate proposals backed by measurements and capability requirements. These are not prerequisites for the CPU graphics subsystem.

**Do not implement all phases in one large commit.** Stop at the agreed milestone; later features are not implicit requirements for an earlier delivery. Run relevant correctness, compatibility, lifecycle, and performance checks throughout implementation.

## Final Acceptance Criteria

The implementation is successful when:

- ffplayout can composite multiple independent graphics layers.
- Existing logo and text features remain compatible.
- The normal YUV video pipeline is preserved.
- Alpha blending is verified against an independent high-precision reference with matching compositing semantics.
- Graphics can be activated, updated, and removed dynamically.
- Animations are deterministic and synchronized to program frames.
- Static graphics are cached efficiently and support configurable fade-in/hold/fade-out without rerasterizing unchanged pixels.
- Animated logos use program-time frame selection and bounded decoding without blocking playout; existing static-logo behavior remains compatible.
- Playout remains operational when graphics rendering fails.
- The subsystem does not require a browser runtime or GPU.
- The architecture can support future rendering and compositing backends without redesigning its scene/control model or requiring CPU readback in every backend.
- Resource budgets, cancellation, stale-result rejection, and shutdown behavior are explicit and tested.
- Shared program graphics scale consistently across renditions, and destination-specific graphics use an explicit composition scope.

Apply these criteria to the completed milestones: animated sources, templates, new API operations, playlist events, and future backends must not block acceptance of the minimal modular core.

## Important Agent Constraints

**Prioritize correctness, maintainability, and broadcast reliability over feature count.**

Do not replace the current working compositor merely because another pixel format or rendering approach appears more conventional.

Investigate first, measure second, and modify only where a clear technical benefit exists.

Preserve ffplayout's CPU-first architecture and existing YUV processing model while establishing a clean path toward higher-quality compositing.
