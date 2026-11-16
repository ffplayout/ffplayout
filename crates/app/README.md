# ffplayout

Application crate and `ffplayout` executable. It owns channel configuration,
SQLite persistence and migrations, playlist scheduling, authentication, the
HTTP API, SSE updates, frontend serving and MoQ integration. Media processing
is delegated to [ff-engine](../engine/README.md).

The main source areas are:

- `api/`: authentication and REST routes.
- `db/`: database models, migrations and data operations in `handles/`.
- `player/`: channel lifecycle, scheduling and graphics control.
- `sse/` and `serve/`: event broadcasting and frontend/static serving.
- `moq/`: relay access and encoded-media publishing.
- `utils/`: application configuration, playlist/file handling and CLI arguments.

Default features enable the GPU desktop output and embedded frontend.
`desktop-cpu` selects the CPU desktop backend when used without the default
desktop feature. `embed_frontend` requires the built frontend assets; see the
[main README](../../README.md) for the complete build setup, including native
FFmpeg libraries.

Run application tests with `cargo test -p ffplayout --no-default-features`.
The HTTP integration suite can be run separately with
`cargo test -p ffplayout --no-default-features --test api_routes`.
