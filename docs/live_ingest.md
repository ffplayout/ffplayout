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
higher priority wins, followed by the stable listener ID. In automatic mode an active takeover is
not pre-empted by another listener. Changes are applied when the configuration is saved,
without restarting the channel playout or its outputs. Names, automatic takeover,
idle timeouts and the ingest log level update live. Priority changes affect the next
source selection and never pre-empt the source already on air.

Enabling or adding an input starts its listener; disabling or removing it stops that
listener. URL, port, protocol, protocol options and demuxer options reopen only the
changed listener. If that listener was on air, playback returns to the playlist at
the current scheduled time. Other listeners and outputs continue running. The
publisher may need to reconnect after a transport change. An unchanged connected
standby input keeps its connection. With no enabled listeners, the relay sleeps
until settings change.

#### Manual live takeover

In **Settings → Ingest**, each listener has its own **Automatically put live inputs on air** switch, enabled by default. Disable it for an input to keep that signal on standby until selected manually. Saving this policy does not require a channel restart and does not interrupt a source already on air. Listener policies are stored per row in `config_live_input.auto_takeover`. Listener IDs remain stable across configuration updates.

The player page shows a small button for each enabled, manually controlled input below
the playout controls. Unlock the controls first. **No signal** is disabled;
**Go live** selects a ready source; **End live** returns to the playlist at the
current scheduled time. Selecting another ready input switches sources without
restarting the outputs. The publisher can remain connected after **End live**.
After a disconnect/idle timeout the playlist resumes. A reconnected publisher on a manual input
needs another manual takeover; it does not automatically return on air. Automatic inputs retain priority-based selection whenever no source is selected. A manual selection can explicitly replace the current source.

Standby sources are continuously drained with bounded buffers. Readiness requires
decoded video, rather than just an accepted connection. Existing decode/input
work continues for connected standby publishers; no extra preview encoder is
created. The existing authenticated playout status stream carries readiness,
session tokens and selection state. There is no additional polling connection.
The status includes every enabled input, even without a publisher, so names and
button availability update in other open player pages too. When playout is stopped,
the stream sends `{"running": false, "ingest_inputs": [...]}` with no ready or
selected input instead of the former plain `not running` message.

Authenticated users with playout-control permissions can send
`PUT /api/control/{channel_id}/ingest` with
`{"listener_id": 1, "session": "<token from ingest_inputs>", "active": true}`.
Set `active` to `false` to end that selection. The command is idempotent, requires manual mode for that input, and
rejects tokens belonging to a previous connection. The public player has no
ingest controls.

When an incoming stream produces video, ffplayout pauses the currently playing
content and switches to the live source. The output remains continuous.

In rare cases, it may happen that, for a short moment after switching, the image freezes, but then it will continue. Also, a brief frame flicker might occur.

#### Live idle timeout

Each RTMP/SRT listener has a **Live idle timeout (seconds)** field in the
frontend. The default is **2 seconds**; existing listeners receive that value
when the database migration runs. The API stores positive integer milliseconds
as `idle_timeout_ms`.

After saving with the normal configuration **Save** button, the new timeout takes
effect immediately without restarting playout or reopening its connection.
Both the listener watchdog and the return to playlist
playback read the same atomic value. The watchdog checks approximately every
100 ms; playlist fallback checks after each iteration with a 10 ms sleep,
subject to frame-processing time. Reducing the timeout can end a connection on
the next check if its current media gap already exceeds the new limit.

The initial wait for decodable frames remains ten seconds. Longer idle timeouts
tolerate longer delivery pauses but also delay recovery when a connected
publisher stops sending. Changes to URL or protocol reopen only that listener; they do not restart playout.

#### Delayed tracks and resource limits

For opt-in RTMP tests that measure resident memory across continuous streaming,
publisher reconnects and watchdog restarts, see [Live-input memory tests](live_memory_tests.md).

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
