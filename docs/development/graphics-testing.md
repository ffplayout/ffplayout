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
