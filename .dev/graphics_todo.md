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
typed engine playback options. Full logo/text controller migration, general
color/plane descriptors and asset preparation scheduling are not complete.
GPU, Servo, templates and new HTTP/frontend graphics APIs are not implemented.

---

## Status reconciliation — 2026-11-02

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
- Implementation/tests in `crates/graphics/src/`, `crates/engine/src/graphics/`,
  and `crates/engine/src/output/composition.rs`.

**Current milestone:** the first minimal scene/layer core is implemented internally
and tested through the CPU compositor. Existing production logo/text paths are
still the compatibility baseline and have not been switched to the new controller.
The engine accepts prepared clip scenes, evaluates their clip-local clock and
passes immutable snapshots through output fan-out. Existing logo/text ordering
and HLS rendition/layout behaviour remain the compatibility baseline. The optional
playlist JSON adapter now prepares static PNG assets asynchronously. Templates,
new HTTP controls, SVG, GPU and Servo remain separate work. Do not repeat the
completed extraction or scheduling/engine integration.

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

**Still open in this milestone:** migrate existing logo/text controls and per-frame
snapshots to the common model, connect preparation/cancellation ownership to the
controller, and complete generic plane/color/capability/layout contracts as needed
by that integration. Group fades currently operate on one already-prepared surface;
no nested-scene renderer or source crossfade is implied.

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
- [ ] Support layer visibility, position, opacity, and transforms.
- [x] Separate scene state/evaluation from rendering; existing production adapters remain to be migrated.
- [x] Separate current logo/text preparation and rendering from final video compositing. General scene state remains open.
- [x] Introduce a bounded, nonblocking internal command interface with explicit acknowledgements/rejections and overload behavior.
- [x] Preserve FIFO command ordering without coalescing in the initial core; reject saturation explicitly.
- [x] Publish immutable scene revisions at an explicit caller-supplied program-frame boundary.
- [ ] Prepare resources before activation; distinguish accepted/loading/ready/active/failed states.
- [ ] Tag preparation/render jobs with resource and scene revisions so stale completions cannot reactivate replaced or cleared graphics.
- [ ] Define cancellation and explicit shutdown/join behavior for preparation workers.
- [ ] Ensure the video processing thread never waits for HTTP, database access, template parsing, or file I/O.
- [x] Support cached static/animated logo sources and prepared text. General multi-layer resource scheduling remains open.
- [ ] Define ownership and lifetime of graphical resources.
- [ ] Ensure errors in individual graphics do not terminate playout.
- [x] Require ready surfaces before take; pending/failed preparation cannot replace an active surface. Missing/unsupported backend resources hide that layer. Keep existing animated-source last-good-frame policy.
- [ ] Never wait for an unfinished render job or backend synchronization on the video thread; consume ready results and apply the late-frame policy.
- [x] Provide bounded ready-surface storage and lease-based retirement; production worker handoff integration remains open.

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

- [ ] Define canvas/reference resolution, coordinate units, anchors, clipping, and fit/aspect rules.
- [ ] Distinguish reference-canvas pixels, normalized coordinates, and explicit destination-pixel sizing; do not mix these implicitly.
- [x] Compose existing logo/text once at the largest HLS rendition and scale smaller renditions from it. Preserve this in the future shared-program layout contract.
- [ ] Treat intentionally destination-specific graphics as a separate composition scope with a concrete layout context.
- [x] Preserve existing logo/text sizing and placement semantics through compatibility adapters; make any behavior change explicit and tested.
- [ ] Keep layout contexts independent of encoders and output protocols. Pipeline integration selects the scope and supplies its dimensions.
- [x] Test current mixed output resolutions/aspect ratios, shared pixels and HLS rendition composition without duplicate logos. General scene-layout tests remain to be added.

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
- [ ] Define explicit pixel-format and plane-layout metadata.
- [ ] Support correct chroma location and subsampling behavior.
- [ ] Preserve color primaries, transfer characteristics, matrix coefficients, and signal range.
- [ ] Design for future 10-bit and HDR support without requiring immediate implementation.
- [ ] Establish explicit handling of straight versus premultiplied alpha.
- [ ] Ensure alpha is not incorrectly applied to offset-encoded YUV components.
- [ ] Describe surfaces with independent format, color, alpha, geometry, and storage metadata.
- [ ] Specify the domain of premultiplication (linear or encoded RGB) explicitly; pixel format alone does not determine alpha semantics.
- [ ] Include full-resolution alpha, visible bounds, plane dimensions/strides, and chroma siting where applicable.
- [ ] Implement owned/shared CPU surfaces first, without making CPU byte access a requirement of every surface.
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
- [ ] Select explicit, cached preparation/conversion paths outside the video hot path where possible; account separately for unavoidable per-frame conversions.
- [ ] Reject unsupported combinations or use a documented fallback; do not silently change color or alpha semantics.
- [ ] Keep fast signal-domain YUV and linear-light RGB as explicit compositing policies with independently measured costs.
- [ ] Allow a future GPU backend to own final compositing as well as graphic rasterization.
- [ ] Do not prescribe mandatory CPU readback, GPU upload, or full-frame RGB conversion in the backend-neutral contract. Actual interop and transfer costs must be evaluated before implementing a GPU path.

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

- [ ] Verify correct source-over alpha blending.
- [x] Support partial transparency and global opacity in the current CPU YUV compositor.
- [x] Preserve transparent/opaque alpha endpoints; skip zero-alpha pixels and cached transparent borders, retaining SIMD handling for visible regions.
- [ ] Correctly account for limited-range versus full-range video.
- [ ] Handle chroma subsampling without introducing unnecessary color bleeding.
- [ ] Correctly handle odd coordinates and nonaligned graphic regions.
- [x] Prepare and reuse 2×2-averaged coverage for the existing 4:2:0 U/V planes. Arbitrary chroma siting remains open.
- [x] Preserve existing performance optimizations.

Pay special attention to chroma blending at partially transparent edges.

### Correctness Reference — Signal-Domain YUV

- [ ] Reuse or extend an independent high-precision reference with the same signal-domain blending, color assumptions, and chroma reduction as the fast path.
- [ ] Compare optimized output against that reference with explicit quantization tolerances.
- [ ] Test transparent-pixel invariance, opaque endpoints, coverage-weighted chroma edges, and overlapping layers.

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
- [ ] Layer opacity.
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
- [ ] Define a reusable source interface that supplies prepared surfaces for a canonical program timestamp. Keep decoder, container, and pixel-storage details behind source adapters.
- [x] Document/test the initial PNG, variable-duration GIF, alpha qtrle video and opaque FFV1 scope. APNG, animated WebP and image sequences remain outside the tested scope; see `docs/animated_logos.md`.
- [x] Preserve source frame durations, including variable durations, and map them to program presentation time rather than advancing one animation frame per video frame.
- [x] Define internal finite/forever source timing and hide/hold-last completion,
  independently from layer fade cycles. Existing animated-logo user settings
  retain their previous infinite-loop behaviour.
- [ ] Integrate finite-repeat controls and explicit source restart with the existing
  source worker; document how restarting source phase interacts with layer retakes.
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
64 MiB/256-frame/5-second caching, bounded streaming lookahead, immutable per-frame
selection before fan-out, error containment and cancellation/join. See
`docs/animated_logos.md` for supported test formats and limitations. The current
single-logo implementation uses one owned worker per channel source. The
backend-independent timing model is implemented, while a general surface/source
contract, shared multi-layer scheduler, finite repeats and graphics commands
remain open. Static assets use a one-time preparation worker that exits after
caching; they retain no decoder or running worker during playback.

This source abstraction should also support future animated lower thirds and
other graphics. It must not depend on Servo or GPU rendering, and it must not
force future sources to return CPU-readable byte buffers.

### Rendering and Caching

- [ ] Rasterize static graphics only when their content changes.
- [x] Cache decoded static images and short logo animations within documented bounds.
- [ ] Cache parsed SVGs when an SVG renderer is introduced.
- [x] Reuse the native font/glyph renderer and prepared text/scaled surfaces. A general bounded text-layout/resource cache remains open.
- [x] Reuse immutable static logo/text surfaces and bounded animated source frames across program frames.
- [ ] Avoid full-scene rerendering when only one layer changes.
- [ ] Minimize unnecessary buffer clears and memory copies.
- [ ] Support dirty-region tracking where beneficial.
- [x] Distinguish rasterization invalidation from video compositing: a cached static surface must still be composited onto every fresh video frame.
- [ ] Include content revision, layout size, font/style dependencies, color/alpha representation, and backend/device identity where relevant in cache keys.
- [ ] Set per-channel/shared cache budgets and limits for asset dimensions, layer counts, and outstanding jobs.
- [ ] Retire unused resources and prevent in-flight jobs from keeping obsolete scenes alive indefinitely.
- [ ] Verify that eviction, rapid replacement, and shutdown release CPU resources and can support future backend-specific retirement rules.

---

## Phase 6 — Deterministic Animation System

**Priority: High**

Animations must use program presentation timestamps rather than wall-clock timing.

Implement:

- [x] Implement backend-independent layer fades for prepared static graphics and
  animated surfaces, including lower thirds. Existing production logo/text fades
  retain their old adapters until integration.
- [ ] Translation and slide animations.
- [ ] Scale animation.
- [ ] Configurable durations.
- [ ] Basic easing functions.
- [ ] Automatic animation completion.
- [ ] Layer replacement and stopping transitions.

Example principle:

```rust
animation_time = current_program_pts - animation_start_pts;
```

### Static graphics and lower-third transition contract

- [ ] Support an explicit entrance → hold → exit lifecycle. Fade durations are
  independent of the image/source duration; a static image remains valid indefinitely.
- [x] Internal transitions use separate fade-in/out durations in integer microseconds
  on program time, with linear easing. Configuration/API adapters will translate
  user-facing seconds. Zero duration gives an immediate take/stop.
- [x] Support indefinite hold until `Stop`, or finite hold followed automatically
  by fade-out, all on program time. Fade cycles may repeat a finite number of
  times or forever only when hold is finite and total cycle duration is positive.
- [x] Begin entrance only when a prepared surface can activate at a frame boundary;
  asset-loading time must not consume its visible fade-in/hold duration.
- [x] Evaluate effective opacity as source alpha × configured layer opacity ×
  transition opacity, applying each factor once. Cache static pixels; changing
  opacity must not trigger rerasterization, decoding or a full-frame RGB conversion.
- [ ] For a composed lower third (background + logo + text), fade the prepared
  group as one graphic (a prepared surface is sufficient for the first milestone;
  a general nested-scene system is not required). Do not independently fade overlapping child layers and
  assume the same result; group compositing/isolation needs its own explicit rule.
- [x] `Stop` starts fade-out from the currently visible transition opacity, even
  during fade-in. A repeated stop must not restart/extend the exit. Retire the
  active presentation at fade completion; reusable cached resources may remain
  within budget. `Clear` removes that layer immediately; shared preparation/cache
  resources may remain ready but cannot activate it automatically. `ClearAll`
  invalidates pending preparations as well.
- [x] Reject taking an already-active layer rather than restarting its fade.
  Layout/opacity updates preserve its presentation clock. Source replacement
  requires explicit clear/take; smooth crossfading two resources remains separate.
- [ ] Integrate explicit source restart and future replacement transitions with
  worker cancellation without reviving retired revisions.
- [x] Keep transition evaluation independent of rasterizer, decoder and compositor
  backend, so future GPU/Servo surfaces can use the same native opacity timeline.
- [ ] Test zero/short durations, different program FPS, partial base opacity,
  stop during entrance, repeated stop/take, clear/replacement while preparing,
  clip/live boundaries and shared HLS rendition scaling. Verify cached static
  pixels are reused and no transition drift or duplicate blending occurs.

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

Minimal native PNG example (see [playlist graphics](../docs/playlist_graphics.md)
for the complete experimental schema):

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

- [ ] Map existing logo control to graphics layers; preserve existing fade semantics.
- [ ] Map existing text overlays to graphics layers.
- [ ] Preserve existing API endpoints.
- [ ] Add graphics state inspection.
- [ ] Allow layer updates and take/stop fades without restarting the playout pipeline.
- [ ] Provide deterministic ordering for concurrent control commands.
- [ ] Define behavior during restart, recovery, and configuration reload.
- [ ] Ensure channel isolation.

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
- [x] Prepare local PNGs off the video thread with one bounded background slot
  per channel. Publish via a nonblocking readiness cell and weak cancellation
  ticket; late results use the current scheduled phase rather than restarting it.
- [x] Keep preparation state/slot on the channel across restarts, cancel publication
  on stop and join tracked work with a bounded wait. Retain the slot/handle after
  timeout until the old job finishes. Keep scene-only limits out of legacy text.
- [x] Preserve effective-item elapsed time independently from source `in` during
  startup/live resume, including still images and live transport playlist items.
- [x] Enforce channel storage containment, symlink checks, file/decoded-size bounds,
  source-surface budgets and per-clip reuse of matching path/size combinations.
- [x] Omit failed sources without disabling unrelated layers. Malformed event
  configuration omits graphics, preserves its JSON and leaves media playing.
- [x] Document usage and limits as experimental in `docs/playlist_graphics.md`.

This first adapter supports static PNGs. A visual frontend editor, templates,
animated playlist sources and migration of all legacy layers into one z-index
domain remain separate work. Existing animated program logos remain supported.
No database migration or new HTTP endpoint is introduced.

---

## Phase 10 — Testing and Performance

**Priority: Critical**

### Visual Correctness

Create reproducible reference test cases for:

- [x] Verify white antialiased text over black with the native renderer; the glyph alpha is applied once.
- [ ] Saturated red text over blue video.
- [ ] Semitransparent gradients.
- [ ] Soft shadows.
- [ ] Overlapping transparent graphics.
- [x] Exercise transparent PNG borders and cached alpha bounds in the existing compositor; broader color-policy references remain open.
- [ ] Odd-positioned graphics on YUV420.
- [ ] Rec.709 limited-range content.
- [ ] Multiple graphics layers with animated opacity.
- [ ] Color-conversion round trips.

Validate each optimized compositor against an independent reference with the same mathematical policy. Separately compare signal-domain YUV and linear-light RGB visually and numerically, without treating their intentional differences as implementation errors.

Use numerical comparisons where appropriate and retain visual regression test images.

### Performance

Benchmark:

- [x] Compare current CPU YUV kernels/compositor with experimental RGBA/linear variants in `overlay_blend` and `frame_blend`; no general scene engine is benchmarked yet.
- [x] Benchmark existing static logo/text composition, including shared-resolution reuse.
- [ ] Animated lower thirds.
- [ ] Multiple overlapping layers.
- [ ] YUVA420 versus YUVA444 graphics preparation.
- [x] Benchmark full-frame work versus cached alpha bounds/ROI clipping.
- [ ] 1080p25, 1080p50, and higher resolutions where supported.
- [ ] Allocation counts per frame.
- [ ] Memory bandwidth and CPU utilization.
- [ ] Long-running playback stability.
- [x] Direct caller-thread SIMD versus persistent worker pools, with wall latency and aggregate CPU measured separately.
- [ ] Idle channels, multiple active channels, rapid updates/clear/reload, cache eviction, failed and cancelled preparation, and bounded memory use.
- [ ] Late/out-of-order renderer results and backend failures: verify uninterrupted video and no resurrection of stale layers.
- [x] Test current shared-resolution reuse, independent output composition and largest-HLS-rendition scaling; extend benchmarks to scene layers when available.
- [x] Test current animated-logo timing across source/program FPS, variable GIF delays, exact fractional loop periods, and existing logo opacity/fades. General scene transitions remain open.
- [ ] Late animation frames, malformed assets, rapid source replacement, and stop/clear during decoding; verify no video-thread waits or obsolete-frame backlog.
- [ ] Long-running animated logo loops: bounded buffers/cache memory, decoder resource release, and no persistent decoding work for static/inactive sources.

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
