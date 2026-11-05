# Animated graphics validation

The graphics tests exercise the production CPU YUVA compositor. They do not
require an encoder, RTMP server, browser or GPU.

## Regression tests

```sh
cargo test -p ff-graphics --features ffmpeg --lib
cargo test -p ff-engine --lib graphics::
```

`overlapping_animation_fades_match_independent_frame_selection_at_multiple_program_rates`
checks three overlapping, staggered GIF presentations with repeating fades and
invisible pauses. At 24/25/30/50/60 program FPS it compares all output planes with
an independently selected fixture frame composed through the existing surface
kernel. The fixture has variable 100/300/200 ms delays. This tests scheduling,
layer order, opacity and pixel compatibility, not an independent alpha formula.
Twenty seconds of program time are evaluated at each frame rate without sleeping.
Cached pixel reservations must remain constant throughout and reach zero afterward.

`repeated_stream_take_stop_clear_and_live_suspension_release_all_resources`
performs 20 source lifecycles with the real local video decoder. Each cycle waits
for a specific requested frame, suppresses demand as during live takeover, checks
that the decoder parks and its cache empties, stops/fades, takes/clears/retakes,
then clears all and joins the worker. A queued snapshot keeps pixels alive until
it is dropped; all pixel, resource and job reservations must then reach zero.
This is an adapter lifecycle test; full ingest switching is covered by engine tests.

Existing tests additionally cover independent streamed presentations, forward and
backward seeks, loop boundaries, finite hide/hold-last, memory pressure, scaled
outputs, queued frame ownership, cancelled publication and preparation failures.

Additional entrance-preparation regressions cover the first scheduled take before
any scene is visible, finite repeat pauses, cancelled/stopped presentations and
future retakes without displacement of an active presentation. The streamed
fixture runs at 10 FPS; a deliberately unaligned repeat boundary makes the first
visible program tick cross into the next source frame. Its selected pixels are
compared with a separate sequential decode, not just checked for availability.
Long gaps empty the decoder cache; a fixed prewarm target must park once ready.

`hq_tagged_sources_keep_metadata_and_blend_in_cached_and_streaming_modes`
prepares resized BT.709 Limited/Full FFV1 alpha sources and a six-second streamed
variant. It checks the actual matrix/range, compares visible output pixels against
a separate sequential decoder and direct blend, and verifies that strict matching
and sRGB-target rejection remain intact. The three small Matroska fixtures derive
from `animated-logo-alpha-ffv1.mkv`: swscale converts input BT.601 Limited to
BT.709 Limited/Full, FFV1 stores `yuva420p`, and explicit matrix/range tags accompany
those pixels. The long version loops the original three frames to six seconds.

`channel_budget_pressure_switches_logo_to_streaming_and_retries_held_snapshots`
reserves 252 MiB of a 256 MiB channel budget for other graphics, then loads a
512×512 video logo with a 128 MiB cache ceiling. Five prepared frames exhaust its
remaining quota. Holding those snapshots blocks further admission; releasing them
must resume the same source at the same requested time, continue through later
frames, stay within the budget and release every lease/job on shutdown.

## Alpha format checks (2026-11-05)

The local graphics adapter was tested against FFmpeg git `46d8f462ee`
(`libavcodec 63.1.102`). The existing Debian static builder uses the same
libavcodec version and includes dav1d 1.5.4. The AVIF stream layout was also
checked with Alpine's FFmpeg 8.0.1. These results are not a compatibility claim
for all FFmpeg versions accepted by ffplayout.

| Source | Result |
| --- | --- |
| VP8/VP9 WebM, `alpha_mode=1` | Transparent, half-transparent and opaque pixels survive graphics decoding, rewind, first-frame loading and composition with the libvpx decoder override. |
| ProRes 4444 MOV | Alpha preserved through the graphics decoder; native decoder selection. |
| FFV1 with alpha in Matroska | Alpha preserved through the graphics decoder; native decoder selection. |
| HEVC auxiliary alpha layer in MP4 | Native `hevc` decoder produces `yuva420p`; alpha survives the graphics adapter and scaling. Initial probing still reports `yuv420p`. |
| Ordinary AV1 video | The static builder automatically selects `libdav1d` and successfully decodes a generated AV1 IVF clip. No explicit AV1 override exists in ffplayout. |
| AVIF with alpha | Associated AV1 color/alpha streams are decoded together. 8-/10-bit stills and a three-frame 25 fps animation preserve 0/128/255 coverage, including scaling, first-frame loading, rewind and seeking. Animation track selection handles FFmpeg exposing both poster items and tracks with duplicate IDs. |
| VVC auxiliary alpha layer | The tested native decoder rejects the generated two-layer bitstream: `Decoding of multilayer bitstreams is not implemented`. VVC alpha is not currently supported. |

`prores_4444_and_ffv1_preserve_transparent_and_semitransparent_pixels` uses
three-frame, 32×32 synthetic fixtures. Their alpha columns alternate between
0/128/255 and 255/128/0. They were encoded with `prores_ks` profile 4, 16-bit
alpha and `yuva444p10le`, and native FFV1 with `yuva420p`, respectively.
VP8/VP9 fixtures use the same synthetic source. The tests compare decoded alpha
values with the source pattern, allowing at most two 8-bit levels of deviation.

The manual HEVC test uses FFmpeg's official
[`hevc/alpha.mp4`](https://fate-suite.ffmpeg.org/hevc/alpha.mp4)
(SHA-256 `e83ed2259c581ae301e4e74b205639f15846e14a01d366e59fa5a2ca646fe5b5`).
It is retained only as a local test download, not added to the repository.
AVIF fixtures were encoded from 32×32 synthetic RGBA PNGs with libavif 1.3.0/aom
3.14.1 (Alpine 3.23):

```sh
avifenc --lossless --speed 8 --jobs 1 alpha.png logo-alpha.avif
avifenc --lossless --speed 8 --jobs 1 --depth 10 alpha.png logo-alpha-10bit.avif
avifenc --lossless --speed 8 --jobs 1 --timescale 25 --duration 1 \
  alpha.png alpha-flipped.png alpha.png animated-logo-alpha.avif
```

`alpha.png` contains alpha columns 0/128/255; the middle animation frame reverses
the columns. The AVIF tests allow at most one 8-bit level of alpha rounding error.
They also blend over an opaque background and check that fully transparent pixels
retain the original video. Metadata-only tests reject unrelated auxiliary types,
wrong item references and invalid sizes/property indices. Explicit premultiplication
is carried to source preparation.

Actual AVIF decoding was tested against a separate local FFmpeg build from
`46d8f462ee` with dav1d 1.5.4. Run the dedicated tests against an FFmpeg installation
with libdav1d or libaom-av1:

```sh
cargo test -p ff-graphics --features ffmpeg --lib avif -- --nocapture
```

The decoding tests print a skip explanation when neither software decoder is
available; the metadata tests run independently of decoder availability.

The VVC fixture was generated with Fluendo's
[`alpha-sei` VVenC branch](https://github.com/fluendo/vvenc/tree/alpha-sei), using
its two-layer alpha/SDI/ACI configuration, three 64×64 frames at 10 FPS and
disabled frame parallelism. This exercises real auxiliary layers rather than
an opaque VVC stream with arbitrary alpha metadata.

The existing general-purpose local macOS FFmpeg build lacks libdav1d and falls
back to its hardware-oriented `av1` decoder, which cannot decode the tested AVIF
here. The additional test build supplied dav1d without changing that installation.
Both provided static Dockerfiles enable libdav1d; shared-library builds depend
on the FFmpeg installation they link against. VVC auxiliary alpha remains unsupported. Encoded-premultiplied AVIF is covered by
the additional preparation tests below.

## Source alpha preparation (2026-11-05)

`ffmpeg/sources/preparation/` normalizes explicitly encoded-premultiplied inputs
and scales alpha graphics in premultiplied 16-bit RGB. The final conversion fuses
RGB unassociation with straight-YUVA quantization; chroma uses coverage-weighted
RGB and never divides offset-encoded YUV by alpha. Existing YUVA blending kernels,
SIMD/Rayon dispatch and scene/control models are unchanged.

Unit tests check zero/tiny alpha, soft coverage, hidden saturated RGB, independent
BT.601 nonuniform-chroma references, limited-range black/chroma offsets, equivalent
straight/premultiplied scaling, mask validation, decoder alpha tags and exact pixel
compatibility of unscaled RGBA/YUVA/opaque sources with the old swscale path.
Alpha tags are detected at build time from FFmpeg headers; older builds do not
reference the newer `AVAlphaMode` fields.

Three small additional AVIF fixtures use libavif 1.3.0/aom 3.14.1 with
`--lossless --premultiply --speed 8 --jobs 1`, plus `--depth 10` for the 10-bit
still or `--timescale 25 --duration 1` for the three-frame animation. The decoder
regression compares final composition against the corresponding straight AVIFs
at native and doubled size, including repeated animation loops. A maximum of
three 8-bit levels accounts for source association quantization and conversion.
These real AVIF tests were run with the separate FFmpeg/dav1d test build described
above; ordinary graphics regressions use the existing FFmpeg installation.

The standalone benchmark includes source conversion, association, scaling,
coverage-aware chroma, allocation/output release and parallel dispatch. It excludes
file I/O, decoding, source-worker coordination, cache lookup and final program
blending. Three warmups precede 21 measured samples. Each configuration runs in a
fresh process; the tables report medians of three runs in shuffled order, including
the median of each run's p95. Measurements used a release build on macOS/aarch64
on 2026-11-05, without concurrent compilation or tests.

```sh
cargo bench -p ff-graphics --features ffmpeg --bench source_preparation -- \
  --width 3840 --height 2160 --mode straight --resize \
  --input rgba8 --execution adaptive --samples 21
```

Modes are `legacy` (old swscale preparation), `straight` and `premultiplied`.
`--resize` halves both dimensions. `--input` accepts `rgba16` (default), `rgba8`
and `yuva8`; source construction/conversion to that input format is outside timing.
`--execution reference` retains the initial f64/swscale alpha-correct path;
`serial` uses SIMD on the caller; `parallel` uses a fixed four-thread pool;
`adaptive` uses production admission and size thresholds. Unscaled straight
sources keep their compatible direct conversion regardless of execution mode.

Wall time is latency; CPU time sums all process threads. Four threads do not
reduce aggregate CPU by four. Peak RSS includes synthetic source construction,
FFmpeg and resize scratch, not just the surface cache; construction temporarily
uses RGBA16 even for 8-bit inputs. Do not compare cross-format RSS as if it were
only preparation workspace.

4K source preparation (all times in milliseconds):

| Source → target | Input / alpha | Reference wall / CPU ms | SIMD serial wall / CPU ms | Four threads wall / CPU ms | Adaptive wall / CPU ms | Adaptive p95 ms | Adaptive peak RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 3840×2160 → 3840×2160 | rgba16 / premultiplied | 20.04 / 20.02 | 5.58 / 5.58 | 1.47 / 5.80 | 1.46 / 5.77 | 1.49 | 93.06 |
| 3840×2160 → 1920×1080 | rgba16 / straight | 30.82 / 30.79 | 18.20 / 18.19 | 5.55 / 18.98 | 5.53 / 18.95 | 5.67 | 190.08 |
| 3840×2160 → 1920×1080 | rgba16 / premultiplied | 25.67 / 25.66 | 14.70 / 14.70 | 3.81 / 15.16 | 3.80 / 15.14 | 3.87 | 126.73 |
| 3840×2160 → 3840×2160 | rgba8 / premultiplied | 37.26 / 37.25 | 6.64 / 6.64 | 2.37 / 6.81 | 2.73 / 7.10 | 2.87 | 125.11 |
| 3840×2160 → 1920×1080 | rgba8 / straight | 48.05 / 48.02 | 18.08 / 18.07 | 5.88 / 19.23 | 5.51 / 19.04 | 5.99 | 159.02 |
| 3840×2160 → 1920×1080 | rgba8 / premultiplied | 42.86 / 42.84 | 15.99 / 15.98 | 5.23 / 16.57 | 4.69 / 16.16 | 5.28 | 158.95 |
| 3840×2160 → 3840×2160 | yuva8 / premultiplied | 31.56 / 31.54 | 18.16 / 18.15 | 14.15 / 18.51 | 14.16 / 18.51 | 14.39 | 113.55 |
| 3840×2160 → 1920×1080 | yuva8 / straight | 42.54 / 42.52 | 29.74 / 29.72 | 17.24 / 30.65 | 17.19 / 30.55 | 17.40 | 147.62 |
| 3840×2160 → 1920×1080 | yuva8 / premultiplied | 37.11 / 37.09 | 27.32 / 27.30 | 16.56 / 27.93 | 16.54 / 27.91 | 16.72 | 147.61 |

1080p source preparation:

| Source → target | Input / alpha | Reference wall / CPU ms | SIMD serial wall / CPU ms | Four threads wall / CPU ms | Adaptive wall / CPU ms | Adaptive p95 ms | Adaptive peak RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1920×1080 → 1920×1080 | rgba16 / straight | 3.04 / 3.04 | 3.04 / 3.04 | 3.03 / 3.03 | 3.04 / 3.04 | 3.08 | 30.72 |
| 1920×1080 → 1920×1080 | rgba16 / premultiplied | 4.97 / 4.96 | 1.35 / 1.35 | 0.36 / 1.42 | 1.35 / 1.35 | 1.38 | 30.23 |
| 1920×1080 → 960×540 | rgba16 / straight | 7.62 / 7.61 | 4.55 / 4.55 | 1.39 / 4.81 | 4.51 / 4.51 | 4.63 | 54.47 |
| 1920×1080 → 960×540 | rgba16 / premultiplied | 6.32 / 6.32 | 3.65 / 3.65 | 0.97 / 3.84 | 3.67 / 3.67 | 3.69 | 38.64 |

For context, the old alpha-incorrect `legacy` RGBA16 scaler measured
8.23 ms wall / 8.23 ms CPU for 4K → 1080p. It is a speed baseline,
not a correctness oracle for transparent edges. The optimized alpha-correct path
still consumes more aggregate CPU than that old scaler. Genuine YUV→RGB remains
with swscale and is a substantial remaining cost for YUVA video sources.

The implementation uses `fast_image_resize` 6.1 without its Rayon feature for
SIMD associated-RGBA16 bilinear convolution (including downscale antialiasing).
Parallel destination bands retain the full source view and filter halo, avoiding
seams. RGB8 expansion is exact byte replication; native premultiplied RGB16 can
borrow the decoded frame without a full-frame copy. Fused quantization uses
packed eight-pixel ARM64 NEON loads or portable runtime SIMD dispatch. Independent
f64 comparisons allow at most one output code-value difference; 16-bit association
is tested for exact rounding. These precision checks do not imply identical
resampling pixels to swscale's different filter implementation.

Production uses the existing shared graphics pool, capped at four threads, for
preparation of at least 3.5 million source/target pixels. Its nonblocking admission
allows one operation; busy callers immediately use serial SIMD. Smaller jobs stay
serial. There are no per-source/output pools. Resize scratch is reused for fixed
geometry and released when dimensions change or the preparer is dropped; serial
and band workspaces count toward the conservative 256 MiB preparation estimate.
Scratch increases peak RSS for scaling compared with the reference path; the
native premultiplied RGB16 path instead removes a full-frame copy.

Static graphics pay preparation once. Animated graphics pay per prepared source
frame, with bounded lookahead and parking when full. At 25 fps, for example,
19 ms aggregate CPU per source frame means about 0.48 CPU core just for preparation,
plus decoding and composition. These synthetic results are neither end-to-end
channel measurements nor guarantees of frame rate or long-run memory stability.

Validation: 135 graphics tests passed (one existing ignored), eight real AVIF
regressions passed with the separate dav1d FFmpeg build, and all-feature/all-target
Clippy passed. Another 52 engine graphics tests and 63 backend-independent graphics
tests passed. Linux x86-64 container checks also passed all 15 preparation tests
and eight real AVIF tests (correctness only, no emulated performance claims).
Tests include reference/portable/NEON comparisons, SIMD tails,
full/limited matrices, alpha extremes, parallel resize halos with varying pixels
on both axes, source immutability, repeated scratch reuse and geometry reset.

Source preparation currently supports encoded SDR only. Linear/HDR normalization,
ICC/gamut conversion and alpha-aware resizing of already prepared YUVA at the
output-cache boundary remain separate work in `.dev/graphics_todo.md`.

## Graphics settings regressions

Quality defaults to high for old playlist JSON. Tests compare Standard scaling
against the legacy scaler using visible pixels (excluding allocation padding),
verify that disabling it never skips premultiplied normalization, and ensure that
asset reuse separates quality modes. Program Logo media keys include quality/cache
while layout-only changes still reuse prepared pixels.

Cache tests check the 128 MiB default, shared setting clones, rejection outside
32–1024 MiB, and persistence across preparation-service resume. The same short GIF
at 2048×2048 switches to streaming with a 32 MiB limit and is fully cached at
128 MiB. The single-frame rewind regression explicitly uses 64 MiB so its boundary
remains stable independently of defaults. Database tests verify migration defaults
and a non-default quality/cache configuration round trip; API restart comparison
excludes both settings while retaining unrelated restart requirements.

## Release load benchmark

Run one configuration per process so process peak RSS is comparable:

```sh
cargo bench -p ff-graphics --features ffmpeg --bench animated_scene -- \
  --width 1920 --height 1080 --fps 25 --layers 3 --seconds 60
```

For a short matrix, including a zero-graphics baseline:

```sh
for canvas in '1920 1080' '3840 2160'; do
  read -r width height <<< "$canvas"
  for fps in 25 50; do
    for layers in 0 3 8; do
      cargo bench -p ff-graphics --features ffmpeg --bench animated_scene -- \
        --width "$width" --height "$height" --fps "$fps" \
        --layers "$layers" --seconds 10
    done
  done
done
```

The loop uses Bash. Repeat the matrix on an otherwise idle machine and vary the
configuration order before drawing performance conclusions. CPU frequency,
thermal conditions and instrumentation affect comparisons.

The scene alternates cached GIF and streamed video sources, each prepared at
320×180, with overlapping positions, 80% layer opacity and repeated fades/pauses.
The measured canvas is 1080p or 4K; overlays are not full-canvas 4K animations.
Source opening/preparation and a short warm-up are excluded from the measurement.
The target frame is reused and reset before every blend.

Reported metrics:

- `frame_p50_ms`, `frame_p99_ms`, `frame_max_ms`: controller advance, source
  selection and composition latency; excludes target reset and pacing sleeps.
  Percentiles use the latest 7200 frames at most; the maximum covers the full run.
- `cpu_percent`: process user+system CPU, including decoder/graphics workers,
  target reset and instrumentation. 100% means one occupied logical CPU.
- `late_frames`: processing crossed the next real-time frame deadline; OS wake-up
  delays can contribute. `--unpaced` instead measures throughput and disables
  deadline counting, so its CPU percentage cannot be compared with paced runs.
- `unavailable_layer_frames`: visible layers for which no surface was composed.
  A streamed frame can be pending after an unplanned take, seek or late preparation. The metric
  prevents missing graphics from silently improving latency results; it does not
  detect a stale last-good frame.
- `blended_layer_frames`: total layers actually composited.
- `rust_allocations_per_frame`, `rust_allocated_bytes_per_frame`: allocation and
  reallocation requests across all Rust threads during the measurement. Native
  FFmpeg allocations are excluded. Allocated bytes are cumulative traffic, not
  retained memory. The counting allocator itself adds overhead.
- `peak_rss_mib`: whole-process high-water mark, including preparation and native
  allocations; bytes on macOS, KiB on Linux are normalized to MiB. CPU/RSS are
  unavailable (`NaN`) on non-Unix targets.
- `max_budget_bytes`, `max_leases`, `max_jobs`: frame-sampled maxima of shared
  graphics reservations, not exact transient peaks or whole-process memory.

Missing/unsupported resources fail the run. Sources without ready pixels are
counted separately. Cleanup must join workers and return every reservation to
zero; otherwise the benchmark exits unsuccessfully.

## Longer stability run

```sh
cargo bench -p ff-graphics --features ffmpeg --bench animated_scene -- \
  --width 3840 --height 2160 --fps 50 --layers 8 --seconds 3600 \
  > graphics-soak.log 2>&1
```

Duration is configurable up to 24 hours. Every minute the probe prints process
CPU seconds, peak RSS and current graphics reservations. Inspect reservation
plateaus and final cleanup separately from peak RSS: an allocator may retain
freed pages. Timing samples use a rolling window of at most 7200 entries (about 57 KiB),
allocated and initialized before measurement. Long runs do not accumulate timing
history or grow their sampling storage.

This is a graphics-only soak. It does not replace full-channel 24/7 tests with
encoding, HLS renditions, live ingest, repeated playlist loading and output failures.

## Baseline before entrance prewarming — 2026-11-03

macOS arm64, optimized Cargo bench profile, one 10-second paced run per
configuration, with the instrumentation described above. These are initial
measurements rather than statistically repeated performance claims. Overlay
size is 320×180, including for 4K canvases. No encoding is included.

| Canvas | FPS | Layers | CPU % | Blend p99 ms | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920x1080 | 25 | 0 | 0.16 | 0.0045 | 11.28 |
| 1920x1080 | 25 | 3 | 0.50 | 0.0970 | 26.34 |
| 1920x1080 | 25 | 8 | 1.14 | 0.2777 | 39.47 |
| 1920x1080 | 50 | 0 | 0.44 | 0.0030 | 11.30 |
| 1920x1080 | 50 | 3 | 1.00 | 0.0985 | 26.88 |
| 1920x1080 | 50 | 8 | 2.36 | 0.2789 | 38.75 |
| 3840x2160 | 25 | 0 | 0.84 | 0.0033 | 20.19 |
| 3840x2160 | 25 | 3 | 1.18 | 0.1219 | 35.73 |
| 3840x2160 | 25 | 8 | 1.38 | 0.2986 | 49.81 |
| 3840x2160 | 50 | 0 | 0.97 | 0.0041 | 20.20 |
| 3840x2160 | 50 | 3 | 2.21 | 0.0963 | 35.72 |
| 3840x2160 | 50 | 8 | 3.52 | 0.3098 | 45.41 |

All twelve runs met their real-time deadlines and released every reservation on
cleanup. Zero-layer runs recorded no Rust allocations in the measurement
window. Streamed sources allocate while decoding; at 50 FPS the three/eight-layer
scenes recorded approximately 4.1/16.2 Rust allocations per program frame across
all threads. This does not mean the blend kernel itself allocates each frame.

Three-layer scenes recorded three missing ready layer-frames; eight-layer scenes
recorded twelve, each at a streamed source's return from an invisible interval.
That is one pending program frame per streamed source and reactivation in these
runs (20 ms at 50 FPS, 40 ms at 25 FPS). Video continued without waiting. These baseline gaps motivated the bounded entrance preparation described below.
These measurements do not establish a worst-case readiness bound.

A separate 180-second paced run rendered 9000 program frames at 4K/50 with eight
layers. It recorded 3.20% process CPU, 0.351 ms p99 / 0.543 ms maximum frame work,
zero missed deadlines and a 49.12 MiB process RSS high-water mark. Current graphics
reservations at both minute samples were 6,336,000 bytes / 40 leases / four jobs;
the frame-sampled maximum was 6,969,600 bytes / 44 leases. Cleanup joined every
worker and returned reservations to zero. There were 212 pending visible
layer-frames over repeated invisible/visible transitions. This short run found
no accumulating graphics reservations; it is not evidence of 24/7 stability.

## Entrance preparation comparison — 2026-11-03

Repeating the 12 configurations with 500 ms entrance preparation recorded no
missing ready layer-frames and no missed real-time deadlines. The eight-layer
4K/50 smoke case recorded 0.291 ms p99 frame work. These remain single-run local
measurements, not statistically established CPU improvements.

The subsequent 180-second run also included bounded ready lookahead selection
for the first visible program tick. Across 9000 frames it recorded:

- Zero missing ready layer-frames (baseline: 212), and zero missed deadlines.
- 0.336 ms p99 / 0.554 ms maximum frame work, 3.14% process CPU.
- 55.03 MiB process RSS high-water mark.
- At most 6,969,600 frame-sampled reserved bytes, 44 leases and four jobs.
- 16.247 Rust allocations per program frame across all threads, including decoding
  and the small ready-window metadata published at entrances.
- Complete worker joining and zero reservations after cleanup.

Pixel buffers are shared with the decoder lookahead, not copied for preparation.
A fixed future request parks after it is ready; longer invisible gaps outside the
lead window still release cache/ready windows. Current layers, including an exit
fade already triggered by Stop, take precedence over future retakes. Unit tests
check source-frame phase independently of the latency probe's availability counter.
The 24/7 full-channel validation remains open.

## Boundary fade regressions

`Presentation::opacity_before_end` is checked against a proportionally fitted
presentation for a 600 ms source with requested one-second entrance/exit fades,
including manual Stop, exact end, already completed presentations, repeat pauses
and a partially truncated final repeat. Earlier complete cycles must keep their
original timing.

CPU compositor pixel comparisons cover the short finite GIF reaching full
opacity at its midpoint and fading before EOF. Additional schedule tests compare
all three output planes for explicit duration beyond clip end and a truncated
last repeat. Application and frontend tests check explicit/legacy durations,
shortened clips, manual Stop and same-layer retakes against the same boundaries.


## Independent signal-domain compositing reference — 2026-11-04

`crates/graphics/src/ffmpeg/compositor/reference_tests.rs` supplies a test-only
f64 source-over oracle independent of production SIMD/scalar kernels, clipping,
alpha-bounds and cached chroma-alpha helpers. It retains floating-point results
between reference layers, normalizes code values with explicit BT.709 limited/full
signal offsets and averages full-resolution alpha before chroma quantization.
It does not compare different signal-domain and linear-light policies.

Run the numerical reference and the whole graphics suite with:

```sh
cargo test -p ff-graphics --features ffmpeg --lib reference_tests -- --nocapture
cargo test -p ff-graphics --features ffmpeg --lib
```

The five tests cover:

- All 65,536 foreground/background byte pairs for eight alpha values and seven
  global opacities: 3,670,016 comparisons against the real-valued equation. Error
  must stay within one byte level; transparent/opaque endpoints must be exact.
- Known BT.709 black/white/red/blue code vectors, matching limited/full-range
  signals, exact transparent/opaque frame endpoints and unchanged target tags.
- A single opaque luma sample at each of the four positions within a chroma cell:
  one-quarter chroma coverage and byte-exact invariance of neighboring cells.
- Saturated red and green surfaces over blue, alpha gradients, transparent borders,
  overlapping layers, global/presentation opacity, even negative/edge placement,
  SIMD tails, padded strides and serial/four-thread/adaptive execution.
- Byte-exact padding invariance. Frame-reference error bounds allow at most two
  luma or three chroma byte levels per layer for sample rounding, alpha rounding,
  presentation-opacity rounding and the production floor of 2×2 coverage.

On the local run, the maximum absolute two-layer errors were Y=1.153428,
U=1.379411 and V=1.172133 byte levels. These are fixture measurements, not a
universal error bound. All five reference tests passed.

Scope: inputs are already prepared straight-alpha YUVA420P with color uniform
within each chroma cell. These tests do not establish RGBA conversion quality,
alpha-weighted chroma preparation for nonuniform cells, mismatched source/target
range or matrix conversion, arbitrary chroma siting, subpixel placement, visual
golden-image correctness, HDR or linear-light composition quality. Those remain
explicitly open in `.dev/graphics_todo.md`. Production rendering is unchanged.
