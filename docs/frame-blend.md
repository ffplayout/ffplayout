# CPU overlay comparison example

`crates/graphics/examples/frame_blend.rs` compares premultiplied source-over
composition of an opaque video frame and an RGBA graphic. It is a standalone
experiment; it does not replace ffplayout's compositor.

## Run

```bash
cargo run --release -p ff-graphics --features ffmpeg --example frame_blend -- \
  --samples 15 --threads 4 --output tmp/frame-blend
```

Defaults use the supplied assets, resolved relative to `CARGO_MANIFEST_DIR`:

- `tests_assets/storage/media_images/pexels-maxi-7339377.png`
- `tests_assets/storage/media_images/lower-third.png`

Both are 3840 x 2160. The video PNG is decoded as opaque sRGB and converted once
to tightly packed 8-bit planar YUV420p (Y, then U, then V), using the selected
matrix, range and transfer. This preparation is outside the blend timers and is
shared by the example and complete-overlay benchmark. The default target is
**BT.709 matrix, limited range, BT.709 transfer**, all explicitly selectable:

```bash
cargo run --release -p ff-graphics --features ffmpeg --example frame_blend -- \
  --matrix bt601 --range full --video-transfer srgb --samples 15
```

The graphic PNG is treated as straight-alpha sRGB with BT.709/sRGB primaries. ICC,
HDR and arbitrary PNG color profiles are not interpreted. If the file was
nonstandardly saved with already-premultiplied sRGB bytes, use
`--input-premultiplied`; it is unassociated before transfer conversion. This
cannot recover precision already lost during 8-bit premultiplication.

`--video` also accepts a raw `.yuv` frame of exactly `width * height * 3 / 2`
bytes. Raw inputs have no color metadata and use the selected color assumptions
directly. Previous numerical results below used the earlier raw fixture; loading
the PNG can change source pixels slightly.

Other options include `--video`, `--graphic`, `--width`, `--height`, `--opacity`,
`--methods` (comma-separated names), and `--no-images`. The latter suppresses PNG and raw YUV image files in all
paths, including both text demos; timing reports are still written. Dimensions must match;
there is no implicit graphic resize. At least three samples are required.
Use a release build; debug builds are unsuitable for performance comparisons.
Full LTO is the workspace default. A faster-build alternative retains release
optimization and can be selected without editing the workspace:

```bash
CARGO_PROFILE_RELEASE_LTO=thin cargo run --release -p ff-graphics \
  --features ffmpeg --example frame_blend -- --samples 15
```

The graphics example and complete-overlay benchmark require the `ffmpeg` feature
for the legacy engine comparison. Their image/SIMD experiments remain development
dependencies of `ff-graphics`; the engine no longer needs those dependencies.

## Compared paths

| Name | Per-frame path |
|---|---|
| `rgba-scalar` | SIMD YUV420 to RGBA8, scalar premultiplied over, SIMD RGBA to YUV420 |
| `rgba-simd` | Same conversions, portable integer SIMD over |
| `rgba-simd-roi` | Same full-frame conversions, blend only the alpha bounding rectangle |
| `rgba-parallel-roi` | Same, blend rows in a fixed-size Rayon pool |
| `tiny-skia` | Same full-frame conversions, tiny-skia premultiplied source-over |
| `linear-pow` | RGBA8, transfer decoding with `powf`, float composition, transfer encoding, YUV420 |
| `linear-lut-simd` | RGBA8, 256-entry inverse-transfer LUT, float SIMD over, 65,536-entry output LUT, YUV420 |
| `linear-lut-roi` | Same full-frame conversions, float over only inside the alpha rectangle |
| `linear-lut-sparse` | Same SIMD RGB8 conversions, but linearization, float over and transfer encoding only inside the alpha rectangle |
| `linear-fused-roi` | Direct YUV-to-float RGB, linearize, compose and convert back in 2x2 blocks only inside the rectangle; no RGBA8 intermediate |
| `linear-fused-lut-roi` | Same fused path, 65,536-entry float transfer tables replace per-pixel powers |
| `linear-fused-lut-parallel` | Same high-precision fused LUT path, disjoint pairs of rows processed by a fixed Rayon pool |
| `linear-fused-lut-simd` | Fused float path with SIMD YUV/RGB matrices, LUT indices, premultiplied over and luma quantization; no RGB8 intermediate |
| `linear-fused-lut-simd-parallel` | Same SIMD path, disjoint row pairs in the fixed Rayon pool |
| `yuva444-scalar` | Expand video chroma to 4:4:4, premultiplied YUVA444 over, then box-reduce chroma |
| `yuva420-scalar` | Blend precomputed premultiplied YUVA420 directly into video planes |
| `yuva420-simd` | Same, portable integer SIMD |
| `yuva420-simd-roi` | Same, only process the even-aligned alpha rectangle |

YUV/RGBA conversion uses `yuv` 0.8.19 with SIMD and the professional RGB-to-YUV
mode. Its optional Rayon support is disabled; the example explicitly benchmarks
parallel blending separately. `rten-simd` selects a supported ISA at runtime;
the example prints the actual backend. The generic integer fallback is scalar.
No nightly compiler or unsafe intrinsics are required in this example.

The scalar fused paths also skip fully transparent pixels inside the rectangle. They
retain precision until final YUV quantization. The lookup version quantizes
transfer-function inputs to 16 bits, not the RGB working buffer to 8 bits.
Each 2x2 block is fully composed before output chroma is averaged.

The high-precision SIMD paths keep an ROI-sized structure-of-arrays copy of the
prepared linear R/G/B source and inverse coverage. Adjacent pixels are SIMD
lanes; video YUV bytes are widened directly to float. There is no intermediate
byte RGB buffer. Chroma is repeated for input 4:2:0 cells and averaged only after
both rows have been composed. Short SIMD tails process whole chroma cells and
never read or write outside their rows.

The transfer tables still use scalar indexed loads: NEON has no arbitrary float
gather, and this portable implementation does not add a custom x86 gather path.
Color matrices, LUT index arithmetic, blending and luma quantization are
explicitly vectorized with `rten-simd`. The final horizontal chroma pair reduction
and byte stores are scalar. This is a SIMD-accelerated pipeline, not a claim that
every instruction is vectorized. The generic ISA is tested against the dispatched
implementation; target-CPU measurements are still needed for other architectures.

## Alpha and color correctness

Alpha is linear coverage. **The colors, not alpha itself, are premultiplied.**
For source-over onto opaque video:

```text
output = source_premultiplied + background * (1 - alpha)
```

Premultiplication is performed in the path's actual working domain:

- Nonlinear RGB: decode the PNG's sRGB transfer, encode into the selected video
  transfer, then premultiply. The RGBA8 paths include 8-bit premultiplication
  quantization, like tiny-skia.
- Linear RGB: decode sRGB first, then premultiply linear colors. Applying an
  inverse transfer directly to premultiplied nonlinear bytes would be wrong.
- YUV: convert unassociated colors to the selected YUV matrix/range, then
  premultiply each **code value**, including its affine offset. Multiplying
  premultiplied RGB by a YUV matrix and adding a full offset regardless of alpha
  would create incorrect black/chroma values at transparent edges.

YUVA420 preparation averages premultiplied chroma and alpha over the same 2x2
block. It does not average straight colors first and multiply by mean alpha.
This prevents hidden RGB colors in transparent texels from bleeding into edges.
Alpha is not applied again in any blend kernel. Effective global opacity is
included in both premultiplied colors and alpha during preparation.

The example chooses a common, explicit chroma model: nearest-neighbour expansion
and box reduction. `yuv420_to_rgba` (not its bilinear variant) uses the matching
nearest reconstruction. Color siting and a sharper reconstruction filter are
separate experiments. Linear here means inverse of the chosen SDR transfer;
it is not an HDR, tone-mapping, BT.1886 display-reference or color-management
implementation. Linear and nonlinear blends intentionally look different,
especially around translucent bright elements and antialiased text.

## Measurements and artifacts

`report.md` and `results.csv` contain kernel median, complete-pipeline median
and p95, single-frame throughput, and quality metrics. When image output is
enabled, each method also produces a full-resolution sRGB preview PNG and a
raw YUV420p output. Three reference PNGs show f64 linear, nonlinear RGB and
native YUV composition.

Timing excludes image reading, PNG decoding, one-time static premultiplication,
lookup creation, thread-pool creation, reference rendering, quality calculations
and file writing. Pipeline timing includes all per-frame conversions, copying,
blending, chroma reduction and quantization back to YUV420p. Kernel-only timing
excludes preparation, with one explicit exception: the fused paths' kernel is
conversion plus composition. Sparse RGBA paths still pay for full-frame color
conversion; sparse fused/YUVA paths avoid it.

All paths warm up, then sampling order rotates. Each sample starts with the
original video, never an already-composited frame. Buffers and the Rayon pool
are reused. Entry points are kept out of line so the optimizer cannot specialize
the separately timed kernel differently from the pipeline's kernel. SIMD/scalar
integer identity, endpoint alpha, transfer functions, mixed-alpha chroma and
the complete paths are covered by tests. A further regression covers full/limited
range, BT.601/BT.709, sRGB/BT.709 transfer, gamut extremes, mixed alpha, short SIMD
tails, generic/native dispatch and sequential/parallel identity:

```bash
cargo test -p ff-graphics --features ffmpeg --example frame_blend
```

The reported static preparation/cache size is deliberately the sum of **all**
comparison representations. Scratch buffers are also shared across all methods.
These numbers are not per-method memory usage or RSS, and should not be used
to estimate the footprint of deploying only one path. Converting a changing
graphic every frame costs additional work; the timings assume a reusable
lower third. For animated graphics, include the relevant preparation stages
in a separate benchmark before making an implementation decision.

PSNR and maximum errors compare Y/U/V **code values** in the even-aligned alpha
rectangle, including chroma. Transparent full-frame background does not inflate
the scores. References use f64 arithmetic:

- RGB-domain methods: RGB conversion/clipping, then composition in the selected
  nonlinear or linear domain, then 4:2:0 conversion.
- Native YUV methods: unclipped YUV composition with full-precision alpha-weighted
  chroma reduction. This preserves input YUV outside the RGB gamut.

Therefore domain PSNR primarily measures the path's quantization error, while
linear-reference PSNR also includes intended differences in blend domain and
gamut clipping. A lower nonlinear-to-linear score alone is not proof of a bug.
PSNR is not a perceptual quality metric; inspect the text, gradients and colored
edges in the PNGs as well. The output is always 8-bit YUV420p, even for float
intermediates. No encoder, GPU, cross-platform speed claim or sustained 24/7
performance is measured. Repeat runs on the target CPU before choosing a path.

## Relevant crates

The experiment directly compares:

- [yuv / yuvutils-rs](https://github.com/awxkee/yuvutils-rs): SIMD YUV/RGB
  conversion, explicit matrix/range and selectable conversion accuracy.
- [rten-simd](https://docs.rs/rten-simd/0.27.0/rten_simd/): portable CPU SIMD with
  runtime dispatch, already used in ffplayout's compositor.
- [tiny-skia](https://github.com/linebender/tiny-skia): premultiplied RGBA CPU
  rasterization and source-over, useful as an independent library comparison.
- [Rayon](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html): a
  reusable fixed worker pool for the parallel ROI method. More threads are not
  automatically faster when conversion or memory bandwidth dominates.
- [image](https://docs.rs/image/latest/image/): PNG loading and preview output;
  its decoding and encoding are excluded from timings.

Additional candidates researched, **not benchmarked here**:

- [pic-scale](https://github.com/awxkee/pic-scale): SIMD scaling, premultiplied
  alpha and linear-light scaling when resizing is also needed. No resizing is
  required for these two matching 4K assets.
- [colorutils-rs](https://github.com/awxkee/colorutils-rs): SIMD color/transfer
  conversions, worth comparing against the explicit LUT path before adding
  another production dependency.

All added dependencies are dev-dependencies, used by examples/tests. No new
runtime output, processing configuration or production compositor is introduced.

## Initial local measurements

One run on macOS/aarch64 with NEON, release optimization, thin LTO, 15 rotated
samples and four Rayon threads produced the following median wall times with
the supplied 4K lower third (alpha rectangle 3438 x 398). These are observations
on this machine, not cross-platform guarantees:

| Path | Blend kernel ms | Complete YUV420p pipeline ms | Maximum domain-reference error |
|---|---:|---:|---:|
| RGBA8 scalar | 3.514 | 7.151 | 2 |
| RGBA8 SIMD | 1.902 | 5.252 | 2 |
| RGBA8 SIMD ROI | 0.330 | 3.685 | 2 |
| Linear powers | 3.831 | 134.518 | 2 |
| Linear LUT, sparse transforms | 0.660 | 7.152 | 2 |
| Linear fused LUT, one thread | 11.002 | 11.236 | 1 |
| Linear fused LUT, four threads | 2.976 | 3.204 | 1 |
| YUVA444 scalar | 2.604 | 9.876 | 1 |
| YUVA420 scalar | 1.329 | 1.524 | 1 |
| YUVA420 SIMD | 0.706 | 0.908 | 1 |
| YUVA420 SIMD ROI | 0.126 | 0.304 | 1 |

The fused LUT output achieved approximately 81 dB PSNR against the linear f64
reference over the overlay rectangle. On this input the one/four-thread fused
LUT outputs were byte-identical; likewise the scalar/SIMD/ROI RGBA outputs and
the scalar/SIMD/ROI YUVA420 outputs matched within their respective families.

For a static broadcast graphic, prepared YUVA420 plus SIMD/ROI is a strong speed
candidate if nonlinear composition is the desired look. For linear-light
quality, the fused float/LUT path avoids the intermediate RGB8 quantization;
parallel execution reduces wall time at the cost of using more CPU cores.
Neither choice should be applied to production solely from one isolated run.

### High-precision SIMD follow-up

The subsequent SIMD comparison used the same 4K assets/color settings on the
same macOS/aarch64 NEON system, thin LTO, 21 rotated samples, and four Rayon
threads. These paths all compose in linear light without an intermediate
RGBA8 video buffer:

| Path | Kernel median ms | Complete pipeline median ms | Pipeline p95 ms |
|---|---:|---:|---:|
| Fused LUT, scalar | 11.185 | 11.451 | 11.696 |
| Fused LUT, SIMD | 5.210 | 5.439 | 5.515 |
| Fused LUT, four threads | 3.011 | 3.230 | 3.299 |
| Fused LUT, SIMD and four threads | 1.381 | 1.586 | 1.619 |

SIMD approximately halved wall time in both comparisons. All four paths
achieved about 81.06 dB PSNR and at most one code value of error against the
f64 linear reference. SIMD and scalar outputs need not be byte-identical because
matrix arithmetic is f32 instead of the scalar path's f64; their rounding
boundaries can differ. The sequential and parallel SIMD outputs were
byte-identical. Performance numbers describe wall time, not aggregate CPU
usage or an encoder's sustainable frame rate.

Reproduce just this comparison:

```bash
CARGO_PROFILE_RELEASE_LTO=thin cargo run --release -p ff-graphics \
  --features ffmpeg --example frame_blend -- \
  --methods linear-fused-lut-roi,linear-fused-lut-parallel,linear-fused-lut-simd,linear-fused-lut-simd-parallel \
  --samples 21 --threads 4 --output tmp/frame-blend-linear-simd
```

## Engine-like text plus lower-third demo

```bash
CARGO_PROFILE_RELEASE_LTO=thin cargo run --release -p ff-graphics \
  --features ffmpeg --example frame_blend -- \
  --text-demo --samples 101 --threads 4 --output tmp/frame-blend-engine-text
```

This produces `composite.png`, `composite.yuv`, three diagnostic text bitmaps,
and `text-blend-report.md`. The three black sentences use 24/12/48 pt at a
defined 96 DPI (32/16/64 raster pixels), with bold and italic spans.
Text is shaped and rasterized by **cosmic-text and Swash**, the same stack as
`crates/graphics/src/rendering/text/mod.rs`, using a shared font system and glyph
cache. The demo uses rich text spans and a prepared linear overlay adapter;
the current production text compositor itself is not replaced.

The lower third and each text line have independent even-aligned bounds and
prepared premultiplied linear planes. All four passes use the high-precision
SIMD kernel directly on the same mutable YUV frame, with no extra full-frame
copy between passes. The in-place path is regression-tested against separate
input/output buffers, including successive overlays reading earlier results.
Keeping these overlays separate avoids processing the large empty region
between the top text and the lower third.

On the local NEON system, 101 warmed samples with four threads measured:

| Operation | Median ms |
|---|---:|
| Lower third | 1.328 |
| Text 24 pt | 0.139 |
| Text 12 pt | 0.038 |
| Text 48 pt | 0.257 |
| **All four passes, measured together** | **1.770** |
| Frame reset/copy plus all passes | 1.970 |

The combined blend p95 was 1.841 ms. The individual median times need not sum
exactly to the independently measured combined median. These are wall times;
they include necessary YUV/linear-RGB transformations, LUT work, blending and
thread-pool work, but exclude rasterization, preparation, frame reset, allocation
and output file I/O. Text is cached as it would be for an unchanged caption;
frequently changing text adds separate shaping/rasterization cost. Font setup,
rasterization, preparation and diagnostic text-image writing took 48.75 ms
once in this run, outside both per-frame timers.


### Compare against the production compositor

```sh
CARGO_PROFILE_BENCH_LTO=thin cargo bench -p ff-graphics --features ffmpeg --bench overlay_blend
```

The existing single-plane scalar/SIMD microbenchmarks are followed by a complete
YUV420 overlay comparison on the supplied 4K video frame. It calls the production
`blend_overlay` implementation directly and compares it with the example's fused
linear-light SIMD kernel called directly on the caller thread, and using persistent
one-worker and four-worker Rayon pools. The direct variant uses the same SIMD
row kernel with a regular iterator and performs no thread-pool dispatch.
Fixtures include synthetic 50×50 and 320×180 logos, a synthetic sparse text-like
1280×96 mask, the actual PNG lower third cropped to its nontransparent bounds,
and a synthetic 1920×1080 overlay. Synthetic text measures coverage behavior,
not font rendering. All fixtures use opacity 1 and share the same straight RGBA
source; preparing YUVA and premultiplied linear RGB happens before measurement.
YUVA is prepared with explicit BT.709 limited-range coefficients for this
comparison, rather than using the different logo/text loading conversions.

For each backend, every sample starts with the original video frame. Resetting,
allocations, LUT/overlay preparation, font rasterization and file I/O are excluded;
per-frame color transforms, SIMD dispatch and thread-pool scheduling are included.
After five warm-ups, 101 samples are measured with rotating backend order, and
median/p95 wall times are printed. Direct, single-worker and four-worker linear
outputs must match exactly. The YUV and linear-light paths deliberately have different color
semantics, so their outputs are not expected to match. These timings compare
steady-state blending cost, not total application CPU or visual equivalence.


Local macOS ARM64 results (101 samples, bench profile with thin LTO), median ms:

| Overlay | Production YUV SIMD | Linear direct, no pool | Linear, 1 worker | Linear, 4 workers |
| --- | ---: | ---: | ---: | ---: |
| Synthetic logo 50×50 | 0.0008 | 0.0220 | 0.0174 | 0.0181 |
| Synthetic logo 320×180 | 0.0085 | 0.5058 | 0.2142 | 0.0758 |
| Synthetic sparse text 1280×96 | 0.0179 | 1.0637 | 0.4323 | 0.1366 |
| Real lower third | 0.1970 | 11.4809 | 4.7442 | 1.2743 |
| Synthetic overlay 1920×1080 | 0.2999 | 18.3624 | 7.5462 | 1.9800 |

The linear kernel now partitions only affected row pairs, rather than distributing
the whole frame and filtering afterwards. This improves load balancing for short
overlays regardless of their vertical position. Even after this adjustment, the
production YUV compositor is substantially faster. Linear light is a possible
quality option, not a demonstrated CPU optimization; four worker threads reduce
latency without establishing lower aggregate CPU cost.


The direct variant calls the same non-inlined SIMD row kernel on the calling
thread; it does not use Rayon. It was slower than a single-worker pool in both
local runs, including after preventing kernel inlining. This is an observed
macOS ARM64 wall-time result, not evidence that dispatch itself makes blending
faster. Caller/worker scheduling and heterogeneous CPU cores have not been
controlled, so these runs cannot isolate pure thread-pool overhead. All three
linear variants produce exactly identical output.

### Render the same scene with the legacy engine method

```sh
CARGO_PROFILE_RELEASE_LTO=thin cargo run --release -p ff-graphics \
  --features ffmpeg --example frame_blend -- \
  --text-demo-yuv --samples 101 --output tmp/frame-blend-engine-yuv
```

This preserves the linear text demo's exact font rasterization, rich-text spans,
24/12/48 pt sizes and placement. Black glyph coverage is prepared as YUVA420 using
the legacy black text values and chroma-alpha reduction. The lower third is loaded
through the production FFmpeg logo decoder with its original conversion defaults.
Both use the production signal-domain YUV compositor, now in `ff-graphics`.
The resulting `composite.png` can be compared with the linear demo image at 100%
zoom. Differences include the original logo conversion as well as signal-domain
versus linear-light alpha blending; this is a comparison of the complete legacy
method, not an isolated change to the blend equation.

An additional logo can be placed over that scene with `--logo PATH`,
`--logo-scale WIDTH:HEIGHT`, `--logo-position 'W-w-12:12'` and
`--logo-opacity 0.7`. These options use the production logo preparation and
compositing path and are available with `--text-demo-yuv`. Supply application
settings from outside the graphics crate; the example does not query the database.
An omitted scale retains the image size subject to the existing even-pixel YUV420
alignment. The reported blend time includes the additional logo.

The text demo supports canvases down to 1280×720. Text sizes and positions scale
from its 3840×2160 reference canvas; supply video and lower-third PNG inputs at
the requested `--width`/`--height`. Additional logo sizing/placement follows its
own supplied settings rather than this text-layout scale.

To compare the same YUV scene with a persistent Rayon pool, add
`--compare-yuv-threads --threads 4`. Each backend starts from the same source
frame, and their execution order alternates across samples. The parallel variant
splits each plane into disjoint row bands, reuses the production SIMD plane
kernel, and preserves layer order. Preparation, frame reset, equality checks and
pool creation are outside the timer; dispatch and synchronization are included.
The comparison runs five strategies: uncropped serial/parallel, cropped
serial/parallel, and the production adaptive path. Visible YUV bytes must match
before timing. Tests cover clipping, strides, vector tails, opacity, transparent
surfaces and conservative pool selection. Unix process CPU time (user + system,
including all workers) is sampled with `getrusage`; those CPU comparisons
currently require macOS/Linux/Unix. Ordinary image rendering needs no CPU sampler.

The production path now caches nontransparent bounds and uses serial SIMD for
regions below 3.5 million visible pixels. Above that threshold a shared pool of
at most four workers can reduce latency, with serial fallback when already busy.
Lower wall latency does not establish lower aggregate CPU usage.

Measured on the same macOS ARM64 host, 1001 interleaved samples with the lower
third, three text lines and database-configured logo:

| Strategy | Wall median ms | CPU median ms/frame |
| --- | ---: | ---: |
| Serial, original extent | 1.229 | 1.230 |
| Rayon 4, original extent | 0.394 | 1.528 |
| Serial, cached bounds | 0.220 | 0.221 |
| Rayon 4, cached bounds | 0.120 | 0.448 |
| Adaptive, cached bounds | 0.218 | 0.219 |

The current scene stays serial after cropping: forcing four workers doubles its
CPU cost for a smaller additional wall-time gain. The image is byte-identical to
the uncropped result. A self-contained size sweep measures the scheduling
crossover without external media fixtures:

```sh
CARGO_PROFILE_BENCH_LTO=thin cargo bench -p ff-graphics --features ffmpeg --bench yuv_execution
```
