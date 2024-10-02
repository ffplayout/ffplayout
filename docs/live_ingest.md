### Live Ingest

With live ingest, you have the possibility to switch from playlist or folder mode to a live stream.

It works by creating an ffmpeg instance in _listen_ (_server_) mode. For example, when streaming over RTMP, you can set the ingest input parameters to:

```
-f live_flv -listen 1 -i rtmp://0.0.0.0:1936/live/my-secrete-streaming-key
```

For SRT you could use:

```
-f mpegts -i 'srt://0.0.0.0:40077?mode=listener&passphrase=12345abcde'
```

Keep in mind that the ingest mode **can't** pull from a server; it can only act as its own server and listen for incoming streams.

When it detects an incoming stream, it will stop the currently playing content and switch to the live source. The output will not be interrupted, so you will have a continuous output stream.

In rare cases, it may happen that, for a short moment after switching, the image freezes, but then it will continue. Also, a brief frame flicker might occur.

You should know that **ffmpeg, in its current version, has no authentication mechanism and simply listens to the protocol and port (no app and stream name).**

ffplayout addresses this issue by monitoring the output from ffmpeg. When the input is **rtmp** and the app or stream name differs from the configuration, it stops the ingest process. So, in a way, we have some control over which streams are accepted and which are not.

In theory, you can use any [protocol](https://ffmpeg.org/ffmpeg-protocols.html) from ffmpeg that supports a **listen** mode.
