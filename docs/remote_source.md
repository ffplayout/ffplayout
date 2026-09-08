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

As an experimental convenience, RTMP, RTSP, SRT, UDP, RTP, RIST, and raw TCP
URLs are treated as time-bounded live playlist entries. Playlist `in` is
ignored, and the source is not reopened for looping. HTTP and HTTPS remain
regular remote-media sources. Live playlist inputs are not an officially
supported workflow and should be tested carefully before production use.
