# API

The API is served by the main ffplayout process. With no command-line
arguments, ffplayout starts the API and frontend on
`http://127.0.0.1:8787`. Set a different address with `--listen` / `-l`:

```bash
ffplayout --listen 127.0.0.1:8787
```

Error bodies are not uniform: authentication handlers commonly return an object
such as `{ "detail": "Incorrect credentials!" }`, while application errors
usually return a JSON string such as `"Access denied"`. Framework rejections
(for example, an invalid JSON body) can use a different response format. Check
the HTTP status first and do not assume every error has a `detail` field.

Unless noted otherwise, `/api` endpoints require:

```http
Authorization: Bearer <ACCESS_TOKEN>
```

`{id}` means a channel ID unless the endpoint identifies another resource
(for example, `/api/user/{id}`). Channel-scoped requests are available only
to users assigned to that channel, unless they are a global admin.

## Authentication and setup

These endpoints do not use a bearer token.

| Method | Endpoint | Request / response |
| --- | --- | --- |
| `POST` | `/auth/login` | `{ "username": "...", "password": "..." }`. Returns `{ "access": "...", "refresh": "..." }`, or a verification message when two-factor authentication is required. |
| `POST` | `/auth/verify` | `{ "username": "...", "code": "123456" }`. Returns access and refresh tokens. Verification codes expire after five minutes. |
| `POST` | `/auth/refresh` | `{ "refresh": "..." }`. Rotates it and returns a new `{ "access": "...", "refresh": "..." }` pair. |
| `POST` | `/auth/logout` | `{ "refresh": "..." }`. Revokes the complete refresh-token family for the session. |
| `GET` | `/api/setup` | Reports whether first-time setup is required. |
| `POST` | `/api/setup` | Completes first-time setup. It works only while no user exists; directory paths must be absolute, writable by the service process, and cannot be protected system paths. |

Access tokens are valid for 45 minutes and refresh tokens for 14 days. Refresh
tokens are single-use: each successful refresh replaces the submitted token.
Reusing an already rotated token revokes the complete token family and requires
a new login.

```bash
curl -X POST http://127.0.0.1:8787/auth/login \
  -H 'Content-Type: application/json' \
  --data '{"username":"admin","password":"<PASSWORD>"}'
```

Example first-time setup:

```bash
curl -X POST http://127.0.0.1:8787/api/setup \
  -H 'Content-Type: application/json' \
  --data '{
    "username":"admin", "mail":"admin@example.invalid", "password":"<PASSWORD>",
    "two_factor":false, "logs":"/var/log/ffplayout",
    "playlists":"/var/lib/ffplayout/playlists",
    "public":"/var/lib/ffplayout/public", "storage":"/var/lib/ffplayout/media",
    "shared":false, "smtp_server":"", "smtp_user":"", "smtp_password":"",
    "smtp_starttls":false, "smtp_port":465
  }'
```

The JWT `secret` is neither exposed nor accepted by the API.
Global setup directory paths cannot be changed through `/api/global`; change
them later locally with `ffplayout -i`. Channel-specific `public`, `playlists`
and `storage` paths can be changed by a global admin through
`PATCH /api/channel/{id}`. Setup, channel and enabled recording destinations
are checked for write access before the corresponding settings are saved.

## Roles

`GA` means global admin, `CA` means channel admin, and `U` means user. All
listed authenticated endpoints also enforce the channel assignment where an
`{id}` is present.

| Access | Meaning |
| --- | --- |
| `GA` | Global admin only |
| `GA, CA` | Global or channel admin |
| `GA, CA, U` | Any authenticated role with access to the target channel |

## Channel and global settings

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/channel/{id}` | `GA, CA, U` | Read one channel. |
| `PATCH` | `/api/channel/{id}` | `GA, CA` | Update a channel. Only a global admin may change its `public`, `playlists`, or `storage` paths. |
| `POST` | `/api/channel` | `GA` | Create a channel. |
| `DELETE` | `/api/channel/{id}` | `GA` | Delete a channel. |
| `GET` | `/api/channels` | `GA, CA, U` | List channels available to the current user. |
| `GET` | `/api/global` | `GA` | Read editable global SMTP and HTTP push settings. Secrets are represented only by `smtp_password_set` and `notification_token_set`. |
| `PUT` | `/api/global` | `GA` | Update SMTP and HTTP push settings. Omit a secret or send an empty value to retain its current value. |

## Playout configuration and capabilities

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/playout/config/{id}` | `GA, CA, U` | Read the complete `PlayoutConfig`. |
| `PUT` | `/api/playout/config/{id}` | `GA, CA` | Replace the complete `PlayoutConfig`. Use the response from `GET` as the request shape. The response contains `requires_restart`. Mail, notification, volume, program-logo and graphics-cache changes are runtime-safe; supported ingest policy/name updates and compatible output updates can also apply without restarting. Other changes may require a restart. Scoped `?scope=ingest`, `?scope=logo` and `?scope=graphics_cache` requests retain unrelated fields; see below. |
| `GET` | `/api/playout/outputs/{id}` | `GA, CA, U` | List configured outputs for the channel. |
| `POST` | `/api/playout/outputs/{id}/{output}/retry` | `GA, CA` | Request reconnection of a failed output destination. No body; returns `true` when accepted. Returns HTTP 409 when the channel is not running or no failed destination can be retried. Acceptance does not mean reconnection has completed. |
| `GET` | `/api/playout/codecs/{id}` | `GA, CA, U` | List supported codecs for HLS, RTMP, SRT, UDP, and custom outputs, including compatible hardware encoders and their curated encoder-setting schema. |
| `GET` | `/api/text/fonts` | `GA, CA, U` | List available font families. |

Scoped configuration updates use the same `PlayoutConfig` request shape as a
full update:

- `?scope=ingest` changes only `ingest` and `logging.ingest_level`.
- `?scope=logo` changes only the program-logo fields in `processing`.
- `?scope=graphics_cache` changes only `processing.graphics_cache_mib`.

See [live ingest](live_ingest.md) and [graphics control](graphics_control.md)
for runtime behavior and the individual graphics settings.

`PUT /api/playout/config/{id}` validates output mode, codecs, HLS subtitle
settings, text preset references, and volume before persisting the change.
Video settings are submitted as `output.video_options`: a string map whose
allowed keys, values, numeric bounds, defaults, and visibility conditions are
provided with the selected video codec by `GET /api/playout/codecs/{id}`.
Audio encoder settings use `output.audio_options` and, for dedicated recording
encoding, `recording.audio_options`. Both are string maps and remain empty by
default so FFmpeg retains its codec defaults. FFmpeg audio-encoding AVOptions
can be submitted through the advanced editor. Options managed by ffplayout,
including bitrate, sample rate, channel layout and time base, cannot be
overridden. FFmpeg validates every remaining name and value
before the configuration is stored, so unsupported options cannot cause a
delayed failure during the next output restart.

Network stream transports use the separate `output.protocol_options` string
map. ffplayout validates its names and values for the RTMP, SRT, UDP, or custom
URL transport and merges accepted values with its enforced I/O timeout when the
output is opened. The map is rejected for HLS, desktop, and local-file outputs.
Values such as SRT passphrases are stored and returned unchanged by these
authenticated endpoints.

`output.metadata_options` is a separate string map of global container tags,
such as `title`, `copyright`, `service_name`, or `service_provider`. Tag names
and values are validated for safe length and characters before saving. They
are applied to the FFmpeg output context before its header is written, not as
muxer or transport options. Whether a tag is retained or visible depends on
the selected output format. Desktop output does not accept container metadata.

For `output.mode: "stream"`, `output.stream_type` also accepts `"custom"`.
Set `output.stream_format` to an FFmpeg output format such as `"decklink"` and
`output.stream_url` to its target, for example a DeckLink device name. The
format must be available in the FFmpeg libraries linked by ffplayout; FFmpeg
performs the final device and codec compatibility check when the playout opens.
Uncompressed video codecs such as `rawvideo` and lossless audio codecs such as
`pcm_s16le` do not use bitrate settings.

## Playout control

| Method | Endpoint | Access | Request body |
| --- | --- | --- | --- |
| `POST` | `/api/control/{id}/text` | `GA, CA, U` | A `TextPreset` object. Send an empty `text` with `use_filename: false` to clear the overlay. |
| `POST` | `/api/control/{id}/playout` | `GA, CA, U` | `{ "control": "back" \| "next" \| "reset" }` |
| `PUT` | `/api/control/{id}/audio` | `GA, CA` | `{ "volume": 0.0 }`, from `0.0` through `1.5`. |
| `PUT` | `/api/control/{id}/ingest` | `GA, CA, U` | `{ "listener_id": 1, "session": "<SESSION>", "active": true }`. Select a ready live input; use `active: false` to end the selection. Returns `"Success"` when accepted. |
| `GET` | `/api/control/{id}/media/current` | `GA, CA, U` | Read the current media and playout state. |
| `POST` | `/api/control/{id}/process` | `GA, CA, U` | `{ "command": "status" \| "start" \| "stop" \| "restart" }` |

During an active live ingest, `back`, `next`, and `reset` return HTTP 200 with
`{ "operation": "ignored", "reason": "live_ingest_active" }`. They do not change
the playlist position or time shift and are not queued for later execution.
Process stop and restart remain available. The `ingest` field in current-media
responses and playout events indicates an actual live takeover, not merely a
listener waiting for a connection.

Manual ingest selection requires automatic takeover to be disabled for that
listener and the current listener's `session` token from `ingest_inputs` in
the playout SSE events. Automatic-mode listeners, stale sessions, unavailable
inputs and attempts while the channel is stopped return HTTP 409. Ending a manual
selection does not disable or disconnect the listener. See
[manual live takeover](live_ingest.md#manual-live-takeover).

```bash
curl -X POST http://127.0.0.1:8787/api/control/1/process \
  -H 'Authorization: Bearer <ACCESS_TOKEN>' \
  -H 'Content-Type: application/json' \
  --data '{"command":"restart"}'
```

## Graphics control (experimental)

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/graphics/{id}` | `GA, CA, U` | Read `revision`, channel `running` state, the program `canvas`, and manual layer settings and status. |
| `POST` | `/api/graphics/{id}` | `GA, CA, U` | Send a graphics action with the latest `revision`. Returns `{ "revision": <NEW_REVISION> }`. |

Actions are `save`, `take`, `update`, `start`, `stop`, `visibility`, `clear` and
`clear_all`. Commands with an outdated revision return HTTP 409. For example,
`{ "action": "stop", "revision": 4, "layer": 1 }` stops layer 1 using revision 4.
Read the current revision before sending a command; the example revision is
not a fixed value. Save/Take requests include the layer's `name` and `settings`.
See [graphics control HTTP API](graphics_control.md#http-api) for action
payloads, persistence, preparation limits and layer-state semantics, and
[playlist graphics](playlist_graphics.md) for clip-local graphics actions.

## Playlists and programme data

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/playlist/{id}?date=YYYY-MM-DD` | `GA, CA, U` | Read a playlist. |
| `POST` | `/api/playlist/{id}` | `GA, CA, U` | Save a complete playlist JSON document. |
| `POST` | `/api/playlist/{id}/generate/{date}` | `GA, CA, U` | Generate and save a playlist. Body is optional: `{ "paths": ["..."], "shuffle": true, "template": { ... } }`. |
| `DELETE` | `/api/playlist/{id}/{date}` | `GA, CA, U` | Delete a playlist. |
| `GET` | `/api/program/{id}` | `GA, CA, U` | Read programme items. Optional `start_after` and `start_before` query parameters accept local ISO date-times. |
| `GET` | `/api/log/{id}` | `GA, CA, U` | Read a log. Optional query parameters: `date`, `timezone`, `download`. |
| `GET` | `/api/system/{id}` | `GA, CA, U` | Read system statistics for the channel. |

Playlist items identify advertisements with `"ad": true`. The field is omitted
for regular content and defaults to `false` when absent. For compatibility,
playlist input may still contain `"category": "advertisement"`; the server
reads it as `"ad": true` and only serializes the new field. Programme responses
expose the resolved state as an `ad` boolean.

## Files

All file-management endpoints require `GA, CA, U` access to the channel.
Paths are resolved below the channel storage root; parent-directory traversal is
rejected.

| Method | Endpoint | Request body or query |
| --- | --- | --- |
| `POST` | `/api/file/{id}/browse` | `{ "source": "", "folders_only": false }` |
| `POST` | `/api/file/{id}/browse-selection` | `{ "source": "folder/clip.mp4", "initial": true }`. List paths for a file/folder picker, without the media-extension filter. |
| `POST` | `/api/file/{id}/media-info` | `{ "source": "folder/clip.mp4" }`. Inspect a selected local file; returns duration and optional video dimensions. |
| `POST` | `/api/file/{id}/create-folder` | `{ "source": "folder" }` |
| `POST` | `/api/file/{id}/rename` | Rename or move a file/folder: `{ "source": "old", "target": "new" }` |
| `POST` | `/api/file/{id}/remove` | `{ "source": "file.mp4", "recursive": false }` |
| `GET` | `/api/file/{id}/upload?path=folder&file_name=clip.mp4&size=1234&batch_id=<ID>` | Returns already received byte ranges for a resumable upload |
| `PUT` | `/api/file/{id}/upload?path=folder` | Chunked `multipart/form-data` upload |
| `PUT` | `/api/file/{id}/import?file=list.m3u&date=YYYY-MM-DD` | `multipart/form-data` with a file field |
| `POST` | `/api/file/{id}/access-token` | `{ "filename": "folder/file.mp4" }` |

The selection endpoints accept paths relative to the storage root or absolute
paths inside that root. Paths outside the root and parent traversal are
rejected. `browse-selection` returns `path`, `breadcrumbs`, `folders`, `files`,
`selected_path` and `fallback`. Entries contain `name` and an absolute `path`;
files also have a `size` in bytes. Hidden entries are omitted. `source` defaults
to an empty string (the storage root), and `initial` defaults to `false`.
With `initial: true`, a file opens its parent directory and sets `selected_path`;
a missing path falls back to its nearest existing directory inside the root
and sets `fallback: true`. Subsequent navigation should use `initial: false`.

`media-info` returns `{ "duration": 12.5, "source_duration": 12.5,
"width": 1920, "height": 1080 }`. Durations are in seconds. If a positive
source duration is unavailable, `duration` defaults to 10 seconds and
`source_duration` is `null`; unavailable dimensions are also `null`.

The upload status response is `{ "received_ranges": [[start, end], ...] }`.
Each `PUT` request contains `fileName`, `start`, `end`, `size`, `batch_id`, and
`chunk` fields. A batch ID identifies concurrent attempts in the running
process. Incomplete data is stored as `<filename>.uploading` with resumable
metadata next to it; the final filename only becomes visible after all byte
ranges have been written successfully.

`POST /api/file/{id}/access-token` returns `{ "access": "...",
"expires_in_seconds": 900 }`. It creates a single-file token bound to the
request IP address and channel. It can be used for browser media previews:

```bash
curl 'http://127.0.0.1:8787/file/1/folder/file.mp4?access=<ACCESS_TOKEN>'
```

`GET /file/{id}/{filename}` also accepts a normal bearer token and supports a
single HTTP `Range` request for seeking.

## Text presets and users

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/presets/{id}` | `GA, CA, U` | List presets for a channel. |
| `POST` | `/api/presets/{id}` | `GA, CA, U` | Create a `TextPreset`. The channel is taken from the path. |
| `PUT` | `/api/presets/{channel}/{preset}` | `GA, CA, U` | Update a `TextPreset`. |
| `DELETE` | `/api/presets/{channel}/{preset}` | `GA, CA, U` | Delete a preset. |
| `GET` | `/api/user` | `GA, CA, U` | Read the current user. |
| `PUT` | `/api/user/{id}` | Self or `GA` | Update a user. Only a global admin can change channel assignments or two-factor settings. |
| `GET` | `/api/user/{id}` | `GA` | Read a user by ID. |
| `DELETE` | `/api/user/{id}` | `GA` | Delete a user. |
| `POST` | `/api/user` | `GA` | Create a user. |
| `GET` | `/api/users` | `GA` | List users. |

## Server-sent events

SSE connections use a short-lived UUID instead of sending a bearer token in a
browser `EventSource` URL.

1. `POST /api/generate-uuid` with a bearer token. It returns `{ "uuid": "..." }`.
2. Optionally validate it with `GET /data/validate?uuid=<UUID>`.
3. Connect to `GET /data/event/{id}?endpoint=playout&uuid=<UUID>` or
   `GET /data/event/{id}?endpoint=system&uuid=<UUID>`.

Add `audio_meter=true` to a playout event URL to request audio-meter updates.

The UUID expires after 30 minutes and is bound to the request IP address.

## MoQ preview (experimental)

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/moq/capabilities` | Public | Reports `{ "available": true }` for MoQ integration. This does not guarantee that a channel output or relay is running. |
| `GET` | `/api/moq/config?channel={id}` | `GA, CA, U` | Obtain private playback configuration and a playback ticket for an enabled MoQ output. Enforces channel access. |
| `GET` | `/moq/config?channel={id}` | Public, when enabled for the channel | Obtain public playback configuration. Returns HTTP 404 when public access is disabled. |
| `GET` | `/moq/relay?channel={id}&ticket=<TICKET>` | Playback ticket, or enabled public access | WebSocket upgrade to the MoQ relay. Public playback can omit `ticket`. |

Bootstrap responses contain `broadcast`, `relay_url`, `relay_port`, `delay_ms`
and `ticket` (`null` for public playback). An unavailable relay returns HTTP
503. Private bootstrap responses must not be cached; obtain a fresh ticket
when reconnecting. See [MoQ output and browser preview](moq.md) for output
configuration, relay transports and access control.

## Public HLS files

HLS playlists, segments, and WebVTT files are public and do not use bearer
authentication:

```text
/public/{channel-id}/live/{playlist-or-segment}
```

For example, the default HLS master playlist of channel 1 is normally:

```text
http://127.0.0.1:8787/public/1/live/master.m3u8
```

Use nginx or another web server for production HLS delivery. The built-in route
is appropriate for previewing and simple deployments.
