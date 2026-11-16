# Workspace crates

The Rust workspace separates application control from media processing and
reusable processing libraries. Run the commands in the individual READMEs from
the repository root.

| Directory | Package | Responsibility |
| --- | --- | --- |
| [app](app/README.md) | `ffplayout` | Application, API, database, playlists and channel lifecycle |
| [engine](engine/README.md) | `ff-engine` | Media input, real-time playback, encoding and output |
| [audio](audio/README.md) | `ff-audio` | PCM processing without FFmpeg dependencies |
| [graphics](graphics/README.md) | `ff-graphics` | Graphics scenes, text rendering and compositing |
| [hls](hls/README.md) | `ff-hls` | Native HLS packaging without FFmpeg dependencies |

`app` uses `engine`, which integrates `audio`, `graphics` and `hls`. The engine
does not depend on the application. Graphics has an optional FFmpeg backend;
audio and HLS operate on caller-provided samples and packets.

See the [main README](../README.md) for installation and build instructions and
[AGENTS.md](AGENTS.md) for backend contribution guidelines.
