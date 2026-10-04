### Live Ingest

With live ingest, you can switch from playlist or folder mode to an incoming live stream.

Configure one or more RTMP or SRT listeners in the channel's playout settings.
For example, an RTMP listen address is:

```
rtmp://0.0.0.0:1936/live/my-secret-streaming-key
```

An SRT listener can use `srt://0.0.0.0:9000`. A sender may publish an MPEG-TS
stream to `srt://YOUR_SERVER:9000?mode=caller`. Configure FFmpeg input options
as key/value pairs on the listener, not in the URL. For encrypted SRT, set
`passphrase` (10–64 bytes) and `pbkeylen` (16, 24, or 32) on both sender and
listener. Listener mode and timeouts remain managed by ffplayout. Passphrases
are stored and returned unchanged through the authenticated configuration API;
restrict configuration access accordingly. Do not expose an unauthenticated SRT
listener directly to an untrusted network; bind it to a trusted interface or
restrict access with a firewall.

Demuxer options are a separate key/value map. By default FFmpeg detects the
input format. On an RTMP listener, `format=live_flv` explicitly uses FFmpeg's
live FLV demuxer; it is not enabled automatically. Other FLV demuxer options
can be set in the same map. For MPEG-TS-specific options on an SRT listener,
set `format=mpegts` as well (for example, `scan_all_pmts=1`). Generic input
probing options such as `probesize` and `analyzeduration` are also supported.
Reducing probing can prevent delayed tracks from being detected.

Ingest mode **can't** pull from a server: it listens for incoming publishers.
Up to eight RTMP/SRT listeners may be configured per channel. Enabled listeners
run at the same time, and each listener of the same protocol needs a distinct
port. Priorities range from 0 to 100. When sources become ready together,
higher priority wins, followed by the stable listener ID. An active takeover is
not pre-empted by another listener. Changing listener settings requires restarting
the channel playout.

When an incoming stream produces video, ffplayout pauses the currently playing
content and switches to the live source. The output remains continuous.

In rare cases, it may happen that, for a short moment after switching, the image freezes, but then it will continue. Also, a brief frame flicker might occur.

#### Live idle timeout

After live frames have started arriving, the idle timeout defaults to **2000 ms**.
Set `FFPLAYOUT_LIVE_IDLE_TIMEOUT_MS` to a positive integer in milliseconds to
override it for all RTMP/SRT listeners in the process:

```sh
FFPLAYOUT_LIVE_IDLE_TIMEOUT_MS=5000 ffplayout -l 0.0.0.0:8787
```

For a systemd installation, add a drop-in with `sudo systemctl edit ffplayout`:

```ini
[Service]
Environment="FFPLAYOUT_LIVE_IDLE_TIMEOUT_MS=5000"
```

Restart ffplayout after changing the variable. It is read once and controls both
the listener watchdog and the idle fallback to playlist/folder playback. Invalid
values, including zero, produce a warning and use the 2000 ms default. The initial
wait for decodable frames remains ten seconds. Longer idle timeouts tolerate longer
delivery pauses but also delay recovery when a connected publisher stops sending.

#### Delayed tracks and resource limits

Live takeover starts with the first decoded video frame. Until then, the playlist
continues and incoming audio is kept in a rolling buffer of at most ten seconds
and 512 decoded frames. Older audio is discarded when either limit is reached;
late video can still trigger takeover without a publisher reconnect.

If an announced audio track is missing or temporarily stops delivering frames,
video can lead audio by up to 2.5 seconds before silence is generated. This grace
period prevents normal transport queueing and millisecond timestamp rounding from
being mistaken for an audio dropout. Late audio is aligned using its timestamps:
samples already replaced by silence are discarded, and partially overlapping
frames are trimmed rather than shifted. Sources without an audio track receive
silence immediately.

If a publisher announces an audio track but sends no audio packets at all during
startup, FFmpeg may wait for the first audio packet while probing the input. Live
takeover then starts only after probing has completed. Normal publishers such as
OBS continuously send audio packets containing digital silence when their audio
input is muted, so this delay should only affect unusual or malformed streams.

At most two readers may exist for the same channel and input URL, including old
readers that have not yet responded to cancellation. If both remain blocked, new
connections wait while playlist playback remains available. While blocked, this
condition is logged as an error at most once every five minutes per input,
including across listener restarts. Recovery is logged when a slot becomes
available. A permanently stuck reader may require restarting the ffplayout
process; restarting only the channel cannot forcibly terminate its thread.
