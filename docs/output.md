ffplayout supports different output modes:

## Stream

The streaming output can be used for classic FFmpeg output URLs such as **RTMP**. Other URL types may work if the linked FFmpeg libraries support the muxer and codec combination, but RTMP is the primary tested streaming target.

**Remember that you need a streaming server as a destination if you want to use this mode.**

Custom FFmpeg output formats and codec combinations, including hardware devices
such as DeckLink, are not all tested by ffplayout. An unsupported format, codec,
pixel format, or device combination can fail when the playout starts. Verify the
combination with the FFmpeg libraries linked by your ffplayout build before
using it in production.

For example, you can use:

- [SRS](https://github.com/ossrs/srs)
- [OvenMediaEngine](https://www.ovenmediaengine.com/ome)
- [Nginx-RTMP](https://www.digitalocean.com/community/tutorials/how-to-set-up-a-video-streaming-server-using-nginx-rtmp-on-ubuntu-20-04)
- [Ant-Media-Server](https://github.com/ant-media/Ant-Media-Server)

Of course, you can also use media platforms that support streaming input.

## Desktop

In desktop mode, ffplayout renders directly through the engine's SDL2 desktop
output. You need a desktop session and a build with the `desktop` feature
enabled; no external **ffplay** process is used.

The desktop window has these controls:

- `F`: toggle fullscreen.
- `Esc`: stop desktop playout.
- Left and right arrow keys: decrease or increase volume. Holding a key repeats the adjustment and shows the volume slider.
- `S`: toggle WebVTT subtitle rendering.
- Double-click the window or use the titlebar fullscreen button: toggle fullscreen.

## HLS

In this mode, ffplayout writes an HLS playlist and media segments into the configured public directory. HLS is commonly used for browser playback and works well with web servers or CDNs.

HLS output is currently the default, mostly because it works out of the box and
doesn't need a streaming target. By default, it writes playlists and segments
to `live/` below the configured channel public directory. The built-in preview
URL is `/public/{channel-id}/live/{playlist}`; for example,
`/public/1/live/master.m3u8`.

The base stream is configured directly in the output settings. Additional HLS variants can be configured as adaptive renditions; they extend the base stream instead of replacing it. A master playlist is generated only when WebVTT subtitles are enabled or when additional variants are configured.

**It is recommended to serve the HLS stream with nginx or another web server,
and not with ffplayout (which is more meant for previewing).**
