## Playlist Generation Template

It is possible to generate playlists based on templates. A template could look like:

```JSON
{
    "sources": [
        {
            "start": "00:00:00",
            "duration": "02:00:00",
            "shuffle": true,
            "paths": [
                "/path/to/folder/1"
            ]
        },
        {
            "start": "02:00:00",
            "duration": "04:00:00",
            "shuffle": false,
            "paths": [
                "/path/to/folder/2",
                "/path/to/folder/3",
                "/path/to/folder/4"
            ]
        },
        {
            "start": "06:00:00",
            "duration": "10:00:00",
            "shuffle": true,
            "paths": [
                "/path/to/folder/5"
            ]
        },
        {
            "start": "16:00:00",
            "duration": "06:00:00",
            "shuffle": false,
            "paths": [
                "/path/to/folder/6",
                "/path/to/folder/7"
            ]
        },
        {
            "start": "22:00:00",
            "duration": "02:00:00",
            "shuffle": true,
            "paths": [
                "/path/to/folder/8"
            ]
        }
    ]
}
```

This can be used as file and run through CLI:

```BASH
ffplayout -g 2023-09-04 - 2023-09-10 --template 'path/to/playlist_template.json'
```

Or through API:

```BASH
curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2023-00-05
    -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
    --data '{"template": {"sources": [\
                {"start": "00:00:00", "duration": "10:00:00", "shuffle": true, "paths": ["path/1", "path/2"]}, \
                {"start": "10:00:00", "duration": "14:00:00", "shuffle": false, "paths": ["path/3", "path/4"]}]}}'
```
