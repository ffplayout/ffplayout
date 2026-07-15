# API

The API is served by the main ffplayout process. By default it listens on
`http://127.0.0.1:8787`. Set a different address with `--listen` / `-l`:

```bash
ffplayout --listen 127.0.0.1:8787
```

All JSON endpoints return an error object with a `detail` field on failure.
Unless noted otherwise, `/api` endpoints require:

```http
Authorization: Bearer <ACCESS_TOKEN>
```

`{id}` always means a channel ID. Channel-scoped requests are available only
to users assigned to that channel, unless they are a global admin.

## Authentication and setup

These endpoints do not use a bearer token.

| Method | Endpoint | Request / response |
| --- | --- | --- |
| `POST` | `/auth/login` | `{ "username": "...", "password": "..." }`. Returns `{ "access": "...", "refresh": "..." }`, or a verification message when two-factor authentication is required. |
| `POST` | `/auth/verify` | `{ "username": "...", "code": "123456" }`. Returns access and refresh tokens. Verification codes expire after five minutes. |
| `POST` | `/auth/refresh` | `{ "refresh": "..." }`. Returns `{ "access": "..." }`. |
| `GET` | `/api/setup` | Reports whether first-time setup is required. |
| `POST` | `/api/setup` | Completes first-time setup. It works only while no user exists. |

Access tokens are valid for three days and refresh tokens for 30 days.

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
| `GET` | `/api/global` | `GA` | Read global settings. The SMTP password is represented only by `smtp_password_set`. |
| `PUT` | `/api/global` | `GA` | Update global settings. Omit `smtp_password` or send an empty value to retain the current password. |

## Playout configuration and capabilities

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/playout/config/{id}` | `GA, CA, U` | Read the complete `PlayoutConfig`. |
| `PUT` | `/api/playout/config/{id}` | `GA, CA` | Replace the complete `PlayoutConfig`. Use the response from `GET` as the request shape. |
| `GET` | `/api/playout/outputs/{id}` | `GA, CA, U` | List configured outputs for the channel. |
| `GET` | `/api/playout/codecs/{id}` | `GA, CA, U` | List supported software codecs for HLS, RTMP, SRT, and UDP. |
| `GET` | `/api/text/fonts` | `GA, CA, U` | List available font families. |

`PUT /api/playout/config/{id}` validates output mode, codecs, HLS subtitle
settings, text preset references, and volume before persisting the change.

## Playout control

| Method | Endpoint | Access | Request body |
| --- | --- | --- | --- |
| `POST` | `/api/control/{id}/text` | `GA, CA, U` | A `TextPreset` object. Send an empty `text` with `use_filename: false` to clear the overlay. |
| `POST` | `/api/control/{id}/playout` | `GA, CA, U` | `{ "control": "back" \| "next" \| "reset" }` |
| `PUT` | `/api/control/{id}/audio` | `GA, CA` | `{ "volume": 0.0 }`, from `0.0` through `1.5`. |
| `GET` | `/api/control/{id}/media/current` | `GA, CA, U` | Read the current media and playout state. |
| `POST` | `/api/control/{id}/process` | `GA, CA, U` | `{ "command": "status" \| "start" \| "stop" \| "restart" }` |

```bash
curl -X POST http://127.0.0.1:8787/api/control/1/process \
  -H 'Authorization: Bearer <ACCESS_TOKEN>' \
  -H 'Content-Type: application/json' \
  --data '{"command":"restart"}'
```

## Playlists and programme data

| Method | Endpoint | Access | Description |
| --- | --- | --- | --- |
| `GET` | `/api/playlist/{id}?date=YYYY-MM-DD` | `GA, CA, U` | Read a playlist. |
| `POST` | `/api/playlist/{id}` | `GA, CA, U` | Save a complete playlist JSON document. |
| `POST` | `/api/playlist/{id}/generate/{date}` | `GA, CA, U` | Generate and save a playlist. Body is optional: `{ "paths": ["..."], "template": { ... } }`. |
| `DELETE` | `/api/playlist/{id}/{date}` | `GA, CA, U` | Delete a playlist. |
| `GET` | `/api/program/{id}` | `GA, CA, U` | Read programme items. Optional `start_after` and `start_before` query parameters accept local ISO date-times. |
| `GET` | `/api/log/{id}` | `GA, CA, U` | Read a log. Optional query parameters: `date`, `timezone`, `download`. |
| `GET` | `/api/system/{id}` | `GA, CA, U` | Read system statistics for the channel. |

## Files

All file-management endpoints require `GA, CA, U` access to the channel.
Paths are resolved below the channel storage root; parent-directory traversal is
rejected.

| Method | Endpoint | Request body or query |
| --- | --- | --- |
| `POST` | `/api/file/{id}/browse` | `{ "source": "", "folders_only": false }` |
| `POST` | `/api/file/{id}/create-folder` | `{ "source": "folder" }` |
| `POST` | `/api/file/{id}/rename` | `{ "source": "old.mp4", "target": "new.mp4" }` |
| `POST` | `/api/file/{id}/remove` | `{ "source": "file.mp4", "recursive": false }` |
| `PUT` | `/api/file/{id}/upload?path=folder` | `multipart/form-data` with a file field |
| `PUT` | `/api/file/{id}/import?file=list.m3u&date=YYYY-MM-DD` | `multipart/form-data` with a file field |
| `POST` | `/api/file/{id}/access-token` | `{ "filename": "folder/file.mp4" }` |

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

The UUID expires after 30 minutes and is bound to the request IP address.

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
