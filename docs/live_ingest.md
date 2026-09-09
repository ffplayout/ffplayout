### Live Ingest

With live ingest, you can switch from playlist or folder mode to an incoming live stream.

The current engine integration provides an RTMP listener. Set the ingest URL to a listen address such as:

```
rtmp://0.0.0.0:1936/live/my-secret-streaming-key
```

Keep in mind that ingest mode **can't** pull from a server; it acts as its own server and listens for incoming RTMP publishers.

When it detects an incoming stream, it will stop the currently playing content and switch to the live source. The output will not be interrupted, so you will have a continuous output stream.

In rare cases, it may happen that, for a short moment after switching, the image freezes, but then it will continue. Also, a brief frame flicker might occur.

#### Delayed tracks and resource limits

Live takeover starts with the first decoded video frame. Until then, the playlist
continues and incoming audio is kept in a rolling buffer of at most ten seconds
and 512 decoded frames. Older audio is discarded when either limit is reached;
late video can still trigger takeover without a publisher reconnect.

If an announced audio track is missing or temporarily stops delivering frames,
video can lead audio by up to 250 ms before silence is generated. Late audio is
aligned using its timestamps: samples already replaced by silence are discarded,
and partially overlapping frames are trimmed rather than shifted. Sources without
an audio track receive silence immediately.

At most two readers may exist for the same channel and input URL, including old
readers that have not yet responded to cancellation. If both remain blocked, new
connections wait while playlist playback remains available. While blocked, this
condition is logged as an error at most once every five minutes per input, including across
listener restarts. Recovery is logged when a slot becomes available. A permanently
stuck reader may require restarting the ffplayout process; restarting only the
channel cannot forcibly terminate its thread.
