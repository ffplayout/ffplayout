### Live Ingest

With live ingest you have the possibility to switch from playlist, or folder mode to a live stream.

It works in a way, that it crate a ffmpeg instance in _listen_ (_server_) mode. For example when you stream over RTMP to it, you can set the ingest input parameters to:

```
-f live_flv -listen 1 -i rtmp://localhost:1936/live/stream
```

Have in mind, that the ingest mode **can't** pull from a server, it only can act as its own server and listen for income.

When it notice a incoming stream, it will stop the current playing and continue the live source. The output will not interrupt, so you have a continuously output stream.

In rare cases it can happen, that for a short moment after switching the image freezes, but then it will continue. Also a short frame flickering can happen.

You need to know, that **ffmpeg in current version has no authentication mechanism and it just listen to the protocol and port (no app and stream name).**

ffplayout catches this problem with monitoring the output from ffmpeg. When the input is **rtmp** and the app or stream name differs to the config it stops the ingest process. So in a way we have a bit control, which stream we let come in and which not.

In theory you can use every [protocol](https://ffmpeg.org/ffmpeg-protocols.html) from ffmpeg which support a **listen** mode.
