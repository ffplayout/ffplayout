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

### Protocol options

Advanced stream settings include a protocol-option map. These values are
applied while FFmpeg opens the network transport and are separate from muxer
options such as MPEG-TS or FLV settings. Common examples are an SRT latency of
`2000000` microseconds and a UDP packet size of `1316` bytes.

Supported transports are RTMP/RTMPS, SRT, UDP, and custom TCP or HTTP(S)
outputs. Available scalar output options are taken directly from the selected
protocol's metadata in the linked FFmpeg build, without a separate option-name
allowlist. Options belonging only to a child protocol are not accepted implicitly.

Custom TLS URLs do not currently support the additional protocol-options field;
existing URLs without these options remain usable.

Unknown options, values outside FFmpeg's supported ranges, protocol mismatches,
and ffplayout-managed timeout options are rejected before the output is
restarted. Protocol options cannot be used for HLS, desktop, or local-file
outputs. SRT passphrases and other values are stored and returned unchanged by
the authenticated configuration API; protect access to ffplayout and its
database accordingly.

Option types, numeric ranges, and symbolic values are validated using the
linked FFmpeg libraries without connecting to the destination. ffplayout also
protects the output lifecycle: listener mode is not allowed in protocol options,
timeouts remain managed, and SRT connection and close waits are limited to ten
seconds. Transport-specific constraints not represented in FFmpeg's metadata
(such as passphrase requirements) are checked only when the transport opens,
not when saving. Unused user options also cause an error on open.
Do not add whitespace to option names. String values,
including leading or trailing spaces in passphrases, are preserved.

For SRT, UDP, and TCP, conflicting values in the URL query and protocol options
are rejected, including supported option aliases. Configure each setting in
only one place, or use identical values in both. HTTP(S) and RTMP(S) queries
remain application parameters and are not compared with protocol options.
Existing URLs without additional protocol options remain unchanged.

## Desktop

In desktop mode, ffplayout renders directly through the engine's native
`winit`/`pixels` output with audio provided by CPAL. `pixels` uses `wgpu` for
GPU-backed YUV-to-RGB conversion, scaling, composition, and presentation. The
desktop renderer uploads Y, U, and V planes directly instead of building a
full-frame CPU RGB buffer. You need a desktop session and a build with the
`desktop` feature enabled; no external **ffplay** process is used.

For systems where the GPU renderer is not usable, build with the optional
`desktop-cpu` feature. It replaces `pixels` with the CPU-only `softbuffer`
renderer:

```bash
cargo build -p ffplayout --features desktop-cpu
```

The desktop window has these controls:

- `F`: toggle fullscreen.
- `Esc`: stop desktop playout.
- Left and right arrow keys: decrease or increase volume. Holding a key repeats the adjustment and shows the volume slider.
- `S`: toggle WebVTT subtitle rendering.
- `E`, `R`, `T`: previous clip, reset the playlist state, next clip.
- `H`: show or hide the desktop keyboard shortcuts.

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
