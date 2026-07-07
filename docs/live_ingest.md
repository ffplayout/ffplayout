### Live Ingest

With live ingest, you can switch from playlist or folder mode to an incoming live stream.

The current engine integration provides an RTMP listener. Set the ingest URL to a listen address such as:

```
rtmp://0.0.0.0:1936/live/my-secret-streaming-key
```

Keep in mind that ingest mode **can't** pull from a server; it acts as its own server and listens for incoming RTMP publishers.

When it detects an incoming stream, it will stop the currently playing content and switch to the live source. The output will not be interrupted, so you will have a continuous output stream.

In rare cases, it may happen that, for a short moment after switching, the image freezes, but then it will continue. Also, a brief frame flicker might occur.
