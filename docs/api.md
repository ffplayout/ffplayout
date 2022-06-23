#### Possible endpoints

Run the API thru the systemd service, or like:

```BASH
ffpapi -l 127.0.0.1:8080
```

For all endpoints an (Bearer) authentication is required.\
`{id}` represent the channel id, and at default is 1.

#### Login is

- **POST** `/auth/login/`\
JSON Data: `{"username": "<USER>", "password": "<PASS>"}`\
JSON Response:
```JSON
{
	"message": "login correct!",
	"status": 200,
	"data": {
		"id": 1,
		"email": "user@example.org",
		"username": "user",
		"token": "<TOKEN>"
	}
}
```

From here on all request **must** contain the authorization header:\
`"Authorization: Bearer <TOKEN>"`

#### User

- **PUT** `/api/user/{user id}`\
JSON Data: `{"email": "<EMAIL>", "password": "<PASS>"}`

- **POST** `/api/user/`\
JSON Data:
```JSON
{
    "email": "<EMAIL>",
    "username": "<USER>",
    "password": "<PASS>",
    "role_id": 1
}
```

#### API Settings

- **GET** `/api/settings/{id}`\
HEADER:
Response is in JSON format

- **PATCH** `/api/settings/{id}`\
JSON Data:
```JSON
    "id": 1,
    "channel_name": "Channel 1",
    "preview_url": "http://localhost/live/stream.m3u8",
    "config_path": "/etc/ffplayout/ffplayout.yml",
    "extra_extensions": ".jpg,.jpeg,.png"
```

#### Playout Config

- **GET** `/api/playout/config/{id}`\
Response is in JSON format

- **PUT** `/api/playout/config/{id}`\
JSON Data: `{ <CONFIG DATA> }`\
Response is in TEXT format

#### Text Presets

- **GET** `/api/presets/`\
Response is in JSON format

- **PUT** `/api/playout/presets/{id}`\
JSON Data:
```JSON
{
    "name": "<PRESET NAME>",
    "text": "<TEXT>",
    "x": "<X>",
    "y": "<Y>",
    "fontsize": 24,
    "line_spacing": 4,
    "fontcolor": "#ffffff",
    "box": 1,
    "boxcolor": "#000000",
    "boxborderw": 4,
    "alpha": "<alpha>"
}

```
Response is in TEXT format

- **POST** `/api/playout/presets/`\
JSON Data: `{ <PRESET DATA> }`\
Response is in TEXT format

#### Playout Process Control

- **POST** `/api/control/{id}/text/`¸
JSON Data:
```JSON
{
    "text": "Hello from ffplayout",
    "x": "(w-text_w)/2",
    "y": "(h-text_h)/2",
     "fontsize": "24",
     "line_spacing": "4",
     "fontcolor": "#ffffff",
     "box": "1",
     "boxcolor": "#000000",
     "boxborderw": "4",
     "alpha": "1.0"
}
```
Response is in TEXT format

- **POST** `api/control/{id}/playout/next/`\
Response is in TEXT format

- **POST** `api/control/{id}/playout/back/`\
Response is in TEXT format

- **POST** `api/control/{id}/playout/reset/`\
Response is in TEXT format

- **GET** `/api/control/{id}/media/current/`\
Response is in JSON format

- **GET** `/api/control/{id}/media/next/`\
Response is in JSON format

- **GET** `/api/control/{id}/media/last/`\
Response is in JSON format

#### Playlist Operations

- **GET** `/api/playlist/{id}/2022-06-20`\
Response is in JSON format

- **POST** `/api/playlist/1/`\
JSON Data: `{ <PLAYLIST DATA> }`\
Response is in TEXT format

- **GET** `/api/playlist/{id}/generate/2022-06-20`\
Response is in JSON format

- **DELETE** `/api/playlist/{id}/2022-06-20`\
Response is in TEXT format

#### File Operations

- **GET** `/api/file/{id}/browse/`\
Response is in JSON format

- **POST** `/api/file/{id}/move/`\
JSON Data: `{"source": "<SOURCE>", "target": "<TARGET>"}`\
Response is in JSON format

- **DELETE** `/api/file/{id}/remove/`\
JSON Data: `{"source": "<SOURCE>"}`\
Response is in JSON format

- **POST** `/file/{id}/upload/`\
Multipart Form: `name=<TARGET PATH>, filename=<FILENAME>`\
Response is in TEXT format
