# ff-engine

FFmpeg-based real-time media engine. It provides probing, file playback, live
listeners, audio/video normalization, graphics integration, encoding, recording
and parallel outputs. Application HTTP routes, database operations and playlist
file management belong to [ffplayout](../app/README.md).

`Playout` and `OutputConfig` are the main playback entry points. Playback and live
controls, output status, graphics controls and media-probing APIs are exported
from `src/lib.rs`.

The main source areas are:

- `input/`: demuxing, decoding and live reception.
- `playout/`: playback timing, transitions and fallback generation.
- `audio_mixer/` and `analysis/`: FFmpeg adapters and integration of
  [ff-audio](../audio/README.md).
- `graphics/`: program, clip and manual graphics adapters for
  [ff-graphics](../graphics/README.md).
- `output/`: encoders, output sharing, desktop display, recording and the
  MPEG-TS writer used by [ff-hls](../hls/README.md).
- `loudness_preview/`: desktop loudness preview and device playback.
- `utils/`: engine configuration, FFmpeg capabilities and media helpers.

Native FFmpeg development libraries are required. Default features enable the
GPU desktop backend. `desktop-cpu` provides a CPU alternative; if both are
enabled, the GPU backend takes precedence. Use `--no-default-features` for
server-only builds. Optional `ffmpeg-filter` and `ffmpeg-device` features enable
the corresponding FFmpeg integrations.

Run server-side tests with `cargo test -p ff-engine --no-default-features --lib`.
The real-time tests can be isolated from other test load with
`cargo test -p ff-engine --no-default-features --lib input::tests::live_reception_ -- --test-threads=1`.
Examples live in `examples/`; `cargo bench -p ff-engine --bench libc_audio`
measures audio processing through the FFmpeg adapter.
