### Possible endpoints

Run the API thru the systemd service, or like:

```BASH
ffplayout -l 127.0.0.1:8787
```

For all endpoints an (Bearer) authentication is required.\
`{id}` represent the channel id, and at default is 1.

#### User Handling

**Login**

```BASH
curl -X POST http://127.0.0.1:8787/auth/login/ -H "Content-Type: application/json" \
-d '{ "username": "<USER>", "password": "<PASS>" }'
```
**Response:**

```JSON
{
    "id": 1,
    "mail": "user@example.org",
    "username": "<USER>",
    "token": "<TOKEN>"
}
```

From here on all request **must** contain the authorization header:\
`"Authorization: Bearer <TOKEN>"`

**Get current User**

```BASH
curl -X GET 'http://127.0.0.1:8787/api/user' -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

**Get User by ID**

```BASH
curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```


```BASH
curl -X GET 'http://127.0.0.1:8787/api/users' -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

**Update current User**

```BASH
curl -X PUT http://127.0.0.1:8787/api/user/1 -H 'Content-Type: application/json' \
-d '{"mail": "<MAIL>", "password": "<PASS>"}' -H 'Authorization: Bearer <TOKEN>'
```

**Add User**

```BASH
curl -X POST 'http://127.0.0.1:8787/api/user/' -H 'Content-Type: application/json' \
-d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
-H 'Authorization: Bearer <TOKEN>'
```


```BASH
curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

#### Settings

**Get Settings from Channel**

```BASH
curl -X GET http://127.0.0.1:8787/api/channel/1 -H "Authorization: Bearer <TOKEN>"
```

**Response:**

```JSON
{
    "id": 1,
    "name": "Channel 1",
    "preview_url": "http://localhost/live/preview.m3u8",
    "extra_extensions": "jpg,jpeg,png",
    "utc_offset": "+120"
}
```

**Get settings from all Channels**

```BASH
curl -X GET http://127.0.0.1:8787/api/channels -H "Authorization: Bearer <TOKEN>"
```

**Update Channel**

```BASH
curl -X PATCH http://127.0.0.1:8787/api/channel/1 -H "Content-Type: application/json" \
-d '{ "id": 1, "name": "Channel 1", "preview_url": "http://localhost/live/stream.m3u8", "extra_extensions": "jpg,jpeg,png"}' \
-H "Authorization: Bearer <TOKEN>"
```

**Create new Channel**

```BASH
curl -X POST http://127.0.0.1:8787/api/channel/ -H "Content-Type: application/json" \
-d '{ "name": "Channel 2", "preview_url": "http://localhost/live/channel2.m3u8", "extra_extensions": "jpg,jpeg,png" }' \
-H "Authorization: Bearer <TOKEN>"
```

**Delete Channel**

```BASH
curl -X DELETE http://127.0.0.1:8787/api/channel/2 -H "Authorization: Bearer <TOKEN>"
```

#### ffplayout Config

**Get Advanced Config**

```BASH
curl -X GET http://127.0.0.1:8787/api/playout/advanced/1 -H 'Authorization: Bearer <TOKEN>'
```

Response is a JSON object

**Update Advanced Config**

```BASH
curl -X PUT http://127.0.0.1:8787/api/playout/advanced/1 -H "Content-Type: application/json" \
-d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
```

**Get Config**

```BASH
curl -X GET http://127.0.0.1:8787/api/playout/config/1 -H 'Authorization: Bearer <TOKEN>'
```

Response is a JSON object

**Update Config**

```BASH
curl -X PUT http://127.0.0.1:8787/api/playout/config/1 -H "Content-Type: application/json" \
-d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
```

#### Text Presets

Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.

**Get all Presets**

```BASH
curl -X GET http://127.0.0.1:8787/api/presets/ -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

**Update Preset**

```BASH
curl -X PUT http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
-d '{ "name": "<PRESET NAME>", "text": "<TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
-H 'Authorization: Bearer <TOKEN>'
```

**Add new Preset**

```BASH
curl -X POST http://127.0.0.1:8787/api/presets/1/ -H 'Content-Type: application/json' \
-d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
-H 'Authorization: Bearer <TOKEN>'
```

**Delete Preset**

```BASH
curl -X DELETE http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

### ffplayout controlling

here we communicate with the engine for:
- jump to last or next clip
- reset playlist state
- get infos about current, next, last clip
- send text to the engine, for overlaying it (as lower third etc.)

**Send Text to ffplayout**

```BASH
curl -X POST http://127.0.0.1:8787/api/control/1/text/ \
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>' \
-d '{"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", fontsize": "24", "line_spacing": "4", "fontcolor": "#ffffff", "box": "1", "boxcolor": "#000000", "boxborderw": "4", "alpha": "1.0"}'
```

**Control Playout**

- next
- back
- reset

```BASH
curl -X POST http://127.0.0.1:8787/api/control/1/playout/ -H 'Content-Type: application/json'
-d '{ "command": "reset" }' -H 'Authorization: Bearer <TOKEN>'
```

**Get current Clip**

```BASH
curl -X GET http://127.0.0.1:8787/api/control/1/media/current
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
```

**Response:**

```JSON
    {
      "media": {
        "category": "",
        "duration": 154.2,
        "out": 154.2,
        "in": 0.0,
        "source": "/opt/tv-media/clip.mp4"
      },
      "index": 39,
      "ingest": false,
      "mode": "playlist",
      "played": 67.808
    }
```

#### ffplayout Process Control

Control ffplayout process, like:
- start
- stop
- restart
- status

```BASH
curl -X POST http://127.0.0.1:8787/api/control/1/process/
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
-d '{"command": "start"}'
```

#### ffplayout Playlist Operations

**Get playlist**

```BASH
curl -X GET http://127.0.0.1:8787/api/playlist/1?date=2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
```

**Save playlist**

```BASH
curl -X POST http://127.0.0.1:8787/api/playlist/1/
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
--data "{<JSON playlist data>}"
```

**Generate Playlist**

A new playlist will be generated and response.

```BASH
curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// --data '{ "paths": [<list of paths>] }' # <- data is optional
```

Or with template:
```BASH
curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2023-00-05
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
--data '{"template": {"sources": [\
           {"start": "00:00:00", "duration": "10:00:00", "shuffle": true, "paths": ["path/1", "path/2"]}, \
           {"start": "10:00:00", "duration": "14:00:00", "shuffle": false, "paths": ["path/3", "path/4"]}]}}'
```

**Delete Playlist**

```BASH
curl -X DELETE http://127.0.0.1:8787/api/playlist/1/2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
```

### Log file

**Read Log File**

```BASH
curl -X GET http://127.0.0.1:8787/api/log/1?date=2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
```

### File Operations

**Get File/Folder List**

```BASH
curl -X POST http://127.0.0.1:8787/api/file/1/browse/ -H 'Content-Type: application/json'
-d '{ "source": "/" }' -H 'Authorization: Bearer <TOKEN>'
```

**Create Folder**

```BASH
curl -X POST http://127.0.0.1:8787/api/file/1/create-folder/ -H 'Content-Type: application/json'
-d '{"source": "<FOLDER PATH>"}' -H 'Authorization: Bearer <TOKEN>'
```

**Rename File**

```BASH
curl -X POST http://127.0.0.1:8787/api/file/1/rename/ -H 'Content-Type: application/json'
-d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: Bearer <TOKEN>'
```

**Remove File/Folder**

```BASH
curl -X POST http://127.0.0.1:8787/api/file/1/remove/ -H 'Content-Type: application/json'
-d '{"source": "<SOURCE>"}' -H 'Authorization: Bearer <TOKEN>'
```

**Upload File**

```BASH
curl -X PUT http://127.0.0.1:8787/api/file/1/upload/ -H 'Authorization: Bearer <TOKEN>'
-F "file=@file.mp4"
```

**Get File**

Can be used for preview video files

```BASH
curl -X GET http://127.0.0.1:8787/file/1/path/to/file.mp4
```

**Get Public**

Can be used for HLS Playlist and other static files in public folder

```BASH
curl -X GET http://127.0.0.1:8787/live/stream.m3u8
```

**Import playlist**

Import text/m3u file and convert it to a playlist
lines with leading "#" will be ignore

```BASH
curl -X PUT http://127.0.0.1:8787/api/file/1/import/ -H 'Authorization: Bearer <TOKEN>'
-F "file=@list.m3u"
```

**Program info**

Get program infos about given date, or current day

Examples:

* get program from current day
```BASH
curl -X GET http://127.0.0.1:8787/api/program/1/ -H 'Authorization: Bearer <TOKEN>'
```

* get a program range between two dates
```BASH
curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T12:00:00&start_before=2022-11-20T11:59:59 \
-H 'Authorization: Bearer <TOKEN>'
```

* get program from give day
```BASH
curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T10:00:00 \
-H 'Authorization: Bearer <TOKEN>'
```

### System Statistics

Get statistics about CPU, Ram, Disk, etc. usage.

```BASH
curl -X GET http://127.0.0.1:8787/api/system/1
-H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
```

