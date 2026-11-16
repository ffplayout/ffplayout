# ff-graphics

Internal graphics library for scene evaluation, text rendering, media preparation
and CPU compositing. It does not depend on the engine, application database,
output protocols or encoders. The crate is not published.

The main source areas are:

- `scene/`: layers, ordering, scopes, control commands and immutable frame evaluation.
- `timing/`: program-time presentation, repeats, fades and animation frame selection.
- `layout/`: reference-canvas geometry and styles.
- `rendering/`: bitmap and text rendering, including SIMD plane blending.
- `resources/`: prepared surfaces, source requests and preparation budgets.
- `ffmpeg/`: optional media decoding, source preparation and CPU frame compositing.

The default feature set has no FFmpeg dependency. The `ffmpeg` feature enables
the backend used by the engine, including animated images/video sources, logo
preparation, text overlays and YUVA compositing onto FFmpeg video frames. Text
layout and rasterization use cosmic-text. CPU execution uses SIMD and Rayon
where supported by the processing path. GPU and browser rendering are future
backend possibilities, not implemented renderers in this crate.

The engine owns playback integration and supplies timestamps, frames and
controls. Scene and timing APIs remain separate from FFmpeg frame handles so
resource preparation and execution can evolve independently.

Run core tests with `cargo test -p ff-graphics` and backend tests with
`cargo test -p ff-graphics --features ffmpeg` (native FFmpeg libraries required).
Use `cargo bench -p ff-graphics --bench scene_evaluation` for scene evaluation,
or `cargo bench -p ff-graphics --features ffmpeg --bench overlay_blend` for
compositing comparisons. The `frame_blend` example compares processing paths;
list its options with
`cargo run -p ff-graphics --features ffmpeg --example frame_blend -- --help`.

See [graphics control](../../docs/graphics_control.md),
[playlist graphics](../../docs/playlist_graphics.md) and
[animated logos](../../docs/animated_logos.md) for application-facing behavior.
