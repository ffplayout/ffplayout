### Possible endpoints

Run the API thru the systemd service, or like:

```BASH
ffpapi -l 127.0.0.1:8000
```

For all endpoints an (Bearer) authentication is required.\
`{id}` represent the channel id, and at default is 1.

#### User Handling

**Login**

```BASH
curl -X POST http://127.0.0.1:8000/auth/login/ -H "Content-Type: application/json" \
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
curl -X GET 'http://localhost:8000/api/user' -H 'Content-Type: application/json' \
-H 'Authorization: Bearer <TOKEN>'
```

**Update current User**

```BASH
curl -X PUT http://localhost:8000/api/user/1 -H 'Content-Type: application/json' \
-d '{"mail": "<MAIL>", "password": "<PASS>"}' -H 'Authorization: <TOKEN>'
```

**Add User**

```BASH
curl -X POST 'http://localhost:8000/api/user/' -H 'Content-Type: application/json' \
-d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
-H 'Authorization: Bearer <TOKEN>'
```

#### ffpapi Settings

**Get Settings**

```BASH
curl -X GET http://127.0.0.1:8000/api/settings/1 -H "Authorization: Bearer <TOKEN>"
```

**Response:**

```JSON
{
    "id": 1,
    "channel_name": "Channel 1",
    "preview_url": "http://localhost/live/preview.m3u8",
    "config_path": "/etc/ffplayout/ffplayout.yml",
    "extra_extensions": "jpg,jpeg,png",
    "timezone": "UTC",
    "service": "ffplayout.service"
}
```

**Get all Settings**

```BASH
curl -X GET http://127.0.0.1:8000/api/settings -H "Authorization: Bearer <TOKEN>"
```

**Update Settings**

```BASH
curl -X PATCH http://127.0.0.1:8000/api/settings/1 -H "Content-Type: application/json"  \
-d '{ "id": 1, "channel_name": "Channel 1", "preview_url": "http://localhost/live/stream.m3u8", \
"config_path": "/etc/ffplayout/ffplayout.yml", "extra_extensions": "jpg,jpeg,png",
"role_id": 1, "channel_id": 1 }' \
-H "Authorization: Bearer <TOKEN>"
```

#### ffplayout Config

**Get Config**

```BASH
curl -X GET http://localhost:8000/api/playout/config/1 -H 'Authorization: <TOKEN>'
```

Response is a JSON object from the ffplayout.yml

**Update Config**

```BASH
curl -X PUT http://localhost:8000/api/playout/config/1 -H "Content-Type: application/json" \
-d { <CONFIG DATA> } -H 'Authorization: <TOKEN>'
```

#### Text Presets

Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.

**Get all Presets**

```BASH
curl -X GET http://localhost:8000/api/presets/ -H 'Content-Type: application/json' \
-H 'Authorization: <TOKEN>'
```

**Update Preset**

```BASH
curl -X PUT http://localhost:8000/api/presets/1 -H 'Content-Type: application/json' \
-d '{ "name": "<PRESET NAME>", "text": "<TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, \
"line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
-H 'Authorization: <TOKEN>'
```

**Add new Preset**

```BASH
curl -X POST http://localhost:8000/api/presets/ -H 'Content-Type: application/json' \
-d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, \
"line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
-H 'Authorization: <TOKEN>'
```

**Delete Preset**

```BASH
curl -X DELETE http://localhost:8000/api/presets/1 -H 'Content-Type: application/json' \
-H 'Authorization: <TOKEN>'
```

### ffplayout controlling

here we communicate with the engine for:
- jump to last or next clip
- reset playlist state
- get infos about current, next, last clip
- send text to the engine, for overlaying it (as lower third etc.)

**Send Text to ffplayout**

```BASH
curl -X POST http://localhost:8000/api/control/1/text/ \
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>' \
-d '{"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", \
    "fontsize": "24", "line_spacing": "4", "fontcolor": "#ffffff", "box": "1", \
    "boxcolor": "#000000", "boxborderw": "4", "alpha": "1.0"}'
```

**Control Playout**

- next
- back
- reset

```BASH
curl -X POST http://localhost:8000/api/control/1/playout/next/ -H 'Content-Type: application/json'
-d '{ "command": "reset" }' -H 'Authorization: <TOKEN>'
```

**Get current Clip**

```BASH
curl -X GET http://localhost:8000/api/control/1/media/current
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

**Response:**

```JSON
{
    "jsonrpc": "2.0",
    "result": {
      "current_media": {
        "category": "",
        "duration": 154.2,
        "out": 154.2,
        "seek": 0.0,
        "source": "/opt/tv-media/clip.mp4"
      },
      "index": 39,
      "play_mode": "playlist",
      "played_sec": 67.80771999300123,
      "remaining_sec": 86.39228000699876,
      "start_sec": 24713.631999999998,
      "start_time": "06:51:53.631"
    },
    "id": 1
}
```

**Get next Clip**

```BASH
curl -X GET http://localhost:8000/api/control/1/media/next/ -H 'Authorization: <TOKEN>'
```

**Get last Clip**

```BASH
curl -X GET http://localhost:8000/api/control/1/media/last/
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

#### ffplayout Process Control

Control ffplayout process, like:
- start
- stop
- restart
- status

```BASH
curl -X POST http://localhost:8000/api/control/1/process/
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
-d '{"command": "start"}'
```

#### ffplayout Playlist Operations

**Get playlist**

```BASH
curl -X GET http://localhost:8000/api/playlist/1?date=2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

**Save playlist**

```BASH
curl -X POST http://localhost:8000/api/playlist/1/
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
-- data "{<JSON playlist data>}"
```

**Generate Playlist**

A new playlist will be generated and response.

```BASH
curl -X GET http://localhost:8000/api/playlist/1/generate/2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

**Delete Playlist**

```BASH
curl -X DELETE http://localhost:8000/api/playlist/1/2022-06-20
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

### Log file

**Read Log Life**

```BASH
curl -X Get http://localhost:8000/api/log/1
-H 'Content-Type: application/json' -H 'Authorization: <TOKEN>'
```

### File Operations

**Get File/Folder List**

```BASH
curl -X POST http://localhost:8000/api/file/1/browse/ -H 'Content-Type: application/json'
-d '{ "source": "/" }' -H 'Authorization: <TOKEN>'
```

**Create Folder**

```BASH
curl -X POST http://localhost:8000/api/file/1/create-folder/ -H 'Content-Type: application/json'
-d '{"source": "<FOLDER PATH>"}' -H 'Authorization: <TOKEN>'
```

**Rename File**

```BASH
curl -X POST http://localhost:8000/api/file/1/rename/ -H 'Content-Type: application/json'
-d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: <TOKEN>'
```

**Remove File/Folder**

```BASH
curl -X POST http://localhost:8000/api/file/1/remove/ -H 'Content-Type: application/json'
-d '{"source": "<SOURCE>"}' -H 'Authorization: <TOKEN>'
```

**Upload File**

```BASH
curl -X POST http://localhost:8000/api/file/1/upload/ -H 'Authorization: <TOKEN>'
-F "file=@file.mp4"
```
