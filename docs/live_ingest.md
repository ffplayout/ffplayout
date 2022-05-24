### Live Ingest

With live ingest you have the possibility to switch from playlist, or folder mode to a live stream.

It works in a way, that it crate a ffmpeg instance in _listen_ (_server_) mode. For example when you stream over RTMP to it, you can set the ingest input parameters to:

```
-f live_flv -listen 1 -i rtmp://localhost:1936/live/stream
```

Have in mind, that the ingest mode **can't** pull from a server, it only can act as its own server and listen for income.

When it notice a incoming stream, it will stop the current playing and continue the live source. The output will not interrupt, so you have a continuously output stream.

In rare cases it can happen, that for a short moment after switching the image freezes, but then it will continue. Also a short frame flickering can happen.

You need to know, that **ffmpeg in current version has no authentication mechanism and it just listen to the protocol and port (no path or app name).**

For security you should not expose the ingest to the world. You localhost only, with an relay/reverse proxy where you can make your authentication. You could also use a [patch](https://gist.github.com/jb-alvarado/f8ee1e7a3cf5e482e818338f2b62c95f) for ffmpeg, but there is no guarantee if this really works.

In theory you can use every [protocol](https://ffmpeg.org/ffmpeg-protocols.html) from ffmpeg which support a **listen** mode.
