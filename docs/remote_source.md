### Video from URL

Videos from a URL are videos that you can watch directly in your browser or download. For example:

```json
    {
        "in": 0,
        "out": 149,
        "duration": 149,
        "source": "https://example.org/big_buck_bunny.webm"
    }
```

This should work in general because most of the time it has duration information and is faster to play than a real live stream source. Avoid seeking, as it can take too much time.

**Live streams as input in playlists, such as RTMP, are not supported.**

Be careful with this; it's better to test it multiple times!
