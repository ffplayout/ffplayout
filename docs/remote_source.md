### Video from URL
Videos from URL are videos where you can watch directly in browser or download, for example:

```json
    {
        "in": 0,
        "out": 149,
        "duration": 149,
        "source": "https://example.org/big_buck_bunny.webm"
    }
```

This should work in general, because most time it have a duration information and it is faster playable then a real live stream source. Avoid seeking because it can take to much time.

**Live streams as input in playlist, like rtmp is not supported.**

Be careful with it, better test it multiple times!
