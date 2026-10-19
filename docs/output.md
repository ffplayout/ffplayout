ffplayout supports different output modes:

## Encoded output pacing

File playback uses one demux worker and separate audio and video workers for
decoding, resampling, scaling and composition. A container is read and demuxed
once; each decoder exclusively owns its codec context. Stream, HLS and CLI null
output also use separate audio and video encoder workers. Slow video codec work
therefore does not directly block audio codec work. Bounded queues apply
backpressure without dropping program packets.

File reading follows a continuous realtime program clock across clip boundaries. Like the current FFmpeg CLI `-re` implementation, the input
governor uses the slowest active stream's normalized DTS, allows an initial
500 ms lead and catches up at at most 1.05 times real time after falling behind.
The reference is FFmpeg's
[`readrate_sleep` implementation](https://github.com/FFmpeg/FFmpeg/blob/46d8f462ee/fftools/ffmpeg_demux.c)
and its [task scheduler architecture](https://github.com/FFmpeg/FFmpeg/blob/46d8f462ee/fftools/ffmpeg_sched.h).
Seek preroll does not count as program progress. Sparse video below two frames
per second allows at least one source frame interval of initial lead so frame
rate conversion can produce the intervening output frames. Live input uses its
source timing; desktop keeps its existing audio-driven rendering clock.

Compressed input queues are limited per track to 256 packets, 16 MiB and two
seconds of timestamp span. Processed frame queues, also used before encoding,
hold at most four video frames / 64 MiB / 200 ms, or 24 audio buffers / 2 MiB /
500 ms. Each bound applies independently. EOF drains delayed codec frames and
resampler samples; skip, restart and errors cancel and join the old clip's
workers before the next clip begins. External audio retains partial buffers
between requests and video loops.

The input coordinator normally keeps audio and video within 100 ms. When the
demuxer is blocked on the only remaining track and the opposite frame queue is
empty, it releases that constraint until the opposite track resumes. This lets
containers without a complete per-stream index reach EOF without a circular
wait between demux, decoding and the coordinator.

The coordinator and encoder handoff wait on queue activity rather than polling
every 10 ms. Queue changes, cancellation and playlist navigation notify a
generation-counted wakeup, so events between checking and waiting are retained.
The input stall deadline remains five seconds. Destination workers likewise wake
for queued media/control, muxer results, manual reconnect or their retry deadline.
Queue count/byte limits are checked before scanning timestamp bounds.

A dedicated output worker sorts compressed packets by DTS across tracks and owns
its destination's mux context. Stream and MoQ workers pace packets against their
media clock to smooth encoder bursts. HLS writes available packets without a second
realtime wait, allowing queued writes to catch up. Its continuous clock still
bounds file reading; parallel outputs retain the common 100 ms encoding lead. The mux
queue allows at most one second between its oldest and newest timestamps, 512 queued
packets and 32 MiB of queued payload, plus the packet currently being written.
Backpressure bounds each destination's delivery work. Parallel outputs isolate
destination errors and compressed-queue overload as described below. Normal finish
drains the queue and writes the trailer.

The mux worker releases the earliest packet once every active audio/video track
has reached its DTS. An idle timeout of 20 ms also releases short tails and live
input without requiring a fixed media lookahead.
Frame-threaded file decoding is limited to at most four threads (fewer at low
frame rates), so decoder-held frames cannot consume the entire read-ahead budget.
Recording keeps its unpaced muxer. Shared encoding owns codecs without opening
an intermediate destination. Parallel output instances are described below.

The program clock persists across clip boundaries and preserves the existing
deadline when its rate changes. Scheduled audio cuts and resampler tails stop
at the sample limit, avoiding an extra video padding frame at each clip boundary.

## Media ownership and modules

`playout/pipeline/` separates orchestration, demux, audio/video processing, the
read-rate governor and scheduling. `output/encoding/` owns codec contexts,
hardware upload, shared audio framing and encoder workers. `EncodingConfig`
describes encoding independently of a recording or transport; the former
`RecordingEncodeConfig` name remains available as an API alias.

`output/shared.rs` distributes encoded packets through bounded destination queues.
`output/delivery.rs` owns each destination's connection lifecycle; `output/mux.rs`
owns ordering, pacing and I/O. `output/diagnostics.rs` retains optional primary
video timing measurements without a Cargo feature. These engine modules do not
depend on MoQ. The application integrates that transport with its relay, importer
and browser endpoints under `crates/app/src/moq/`.

Normal finish closes producers, drains accepted media, flushes codec tails and
joins the auxiliary transport workers before the MoQ importer reaches EOF.
Dropping an unfinished output cancels it and joins its workers. The importer
also has separate drain and abort paths, with a five-second shutdown deadline.

## Null (CLI only)

Use `--output null` to run the normal stream encoding pipeline in real time
without a network destination or an output file. FFmpeg's null muxer discards
the encoded packets. This process-local override uses the primary output's
effective encoding settings, ignores its network/muxer options and does not update the DB.
Starting without `--output` restores the stored active output. The null mode is
not available through the frontend or configuration API.

MoQ is a regular output instance. Add it under **Outputs**, select own encoding or
an existing output/variant, and save to apply it to the running parallel pipeline. CLI null output
remains a temporary override of the entire configured output collection.

## Stream

The streaming output can be used for classic FFmpeg output URLs such as **RTMP**. Other URL types may work if the linked FFmpeg libraries support the muxer and codec combination, but RTMP is the primary tested streaming target.

**Remember that you need a streaming server as a destination if you want to use this mode.**

For a browser preview using Media over QUIC with WebTransport/WebSocket, see
[MoQ output and player](moq.md). MoQ is built in and supports packet reuse or
separate encoding. Internal and public playback share one Vue component.

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

### Container metadata

The advanced output settings accept global container metadata as key/value
pairs. For example, `title` and `copyright` can identify a recording or stream.
For MPEG-TS, set `service_name` and `service_provider` to advertise the DVB
service description. These tags are separate from muxer and protocol options.
An empty map preserves FFmpeg's defaults. Which other tags survive depends on
the selected muxer; HLS playlist tags are not generated automatically from
container metadata.

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

The integrated MoQ output provides a public player at `/moq/player?channel=1`
when public access is explicitly enabled for that output (off by default).
See [Experimental MoQ preview](moq.md) for configuration and transport.

## Multiple output instances

The **Outputs** configuration tab presents up to eight output instances as
stacked cards. Each card has a name, an enabled flag, runtime status, encoding
settings and destination options. Disabled cards retain their configuration.
HLS, Stream and MoQ can be added, edited, enabled, disabled or removed while the
parallel pipeline runs. Saving changes updates the affected destinations at the
next processed media frame; a paused pipeline applies them when playback resumes.
Other encoding groups continue. Migration 00007 preserves
existing selections and consolidates the new output-instance schema.

**General → Processing** defines the shared program resolution and frame rate.
Live inputs have their own **Ingest** tab. **Outputs** appears below **Recording**
in the settings navigation.
Each output can inherit the program resolution or select a different resolution.
Desktop dimensions set the window size; the renderer scales the shared program.
All outputs use the program frame rate. The API retains `output` and `outputs`
for compatibility, alongside an optional `program` format; the first enabled
output occupies the compatibility slot without appearing as primary in the UI.

Each instance has its own settings and delivery worker. All instances receive
one shared, processed program: playlist decisions, live takeover, overlays,
volume and loudness processing run once. Output resolutions may differ; frame
rates always match the shared program.

Each encoded output selects **Own encoding** or an **Encoding source**. Choosing
a saved Stream, MoQ or HLS output with its own encoding reuses its compressed video/audio
packets. HLS sources expose their base rendition and additional variants. Only
one HLS output instance is supported per channel; adaptive renditions belong to
that output. Own encoding remains independent even when another output happens
to use identical settings.

Dependencies resolve to the original encoder and rendition. Self references,
cycles, missing sources and incompatible container/codec combinations
are rejected. The UI offers compatible saved sources. Destination URLs, container
metadata and transport settings remain independent. A common closed GOP fits the
participating HLS segment intervals; codec headers support each container.

Adaptive HLS and WebVTT can supply shared encoded media. Packet-copy recording
can consume an output source without another encoder. Packet-copy recording
receives media directly from the encoding group and
continues through destination failures and reconnections without reopening its
recording muxer. Dedicated recording encoding remains separate. MoQ uses the same
`config_output` table as other outputs; no separate preview configuration remains.

`output/parallel.rs` owns the program clock and sends referenced program frames
to the encoding groups. Each raw queue is limited to 128 messages, 64 MiB and one
second of media. Shared profiles reference their compressed packets into separate
delivery queues, each limited to 512 messages, 32 MiB and one second. Stream/MoQ
then use the bounded paced mux queue. HLS writes directly in its isolated delivery
worker without another queue or pacing stage. No destination owns the shared encoder.

With a visible logo, video is first scaled to the effective encoding resolution,
then composited. The decoded logo retains its configured pixel dimensions; its
position is evaluated against that output's width/height, keeping pixel offsets
from the edges. Encoders with the same resolution reuse the composited pixels
for that program frame, including matching HLS renditions. Other resolutions
have independent scaler/compositor locks. The cache releases each image after
its last encoder takes a reference and is bounded by the existing frame lifetime;
live output changes clear unused scaler states. Hidden logos retain the direct
encoder scaling path. Shared-encoding destinations receive the already composited
compressed stream and never blend the logo again.
Opening, transport or mux failures disable only the affected destination; other
destinations keep using the profile. Encoder failure or raw queue overload affects
the whole encoding group. Compressed queue overload disables that destination
instead of resuming with a broken GOP. Destination failures retry 15 seconds after
each failed attempt, for at most three minutes after the first failure. No new
attempt starts at or after that deadline; blocking network I/O retains its ten-second
timeout. A successful mux write resets the failure window, so a later outage gets
a new three-minute window. After exhaustion the output stays failed. **Reconnect**
starts a fresh round for that destination using its running configuration, without
restarting the channel or other outputs. Output settings use the live update path.
Encoder failures require recreating the affected encoding group (for example by
disabling its consumers and enabling them again) or restarting the channel; the button is
only available for live destination workers. Even if every destination is offline,
the program and encoders continue and discard outgoing packets. Reconnected
outputs discard stale queued media and wait for a fresh keyframe for each rendition;
audio older than that keyframe is omitted. Normal shutdown drains the encoders before closing all
packet queues and joining delivery workers; abort cancels the outstanding work.

With desktop enabled, desktop audio drives program timing; at most one desktop
instance is allowed. Its frame sender distributes accepted program frames to the
other workers, including the logo, while the renderer retains its existing logo
path. Without desktop, the shared program clock permits 100 ms of encoding lead;
destination pacing and network I/O cannot throttle healthy branches. The application
keeps the program rate at 1.0; HLS no longer changes it based on playlist delta.
Short HLS write stalls are absorbed by bounded queues and caught up as data becomes
available. Sustained overload still exceeds those bounds; the existing configured
playlist stop threshold remains responsible for excessive schedule drift.

HLS playlists and segments retain the `live/` directory beneath the channel's
public directory. Recording copy sources refer to their enabled output instance;
dedicated recording remains a separate feature.

MoQ is always available as mode `moq`. It can be the only output and publishes to
the `/moq/player?channel=ID` browser player. It currently supports one
MoQ instance per channel, H.264/H.265 with AAC, and requires `--listen` to start
the relay. Encoder names are unrestricted in the database; runtime codec
capabilities reflect what the current MPEG-TS importer supports. AV1 needs an
import/container path that supports it before it can be enabled.

Private MoQ publication starts on Player demand and stops three minutes after the
last authenticated relay connection ends. Public MoQ publishes continuously while
enabled. Sleeping MoQ releases its delivery/importer and unneeded encoder group;
other outputs and recording retain any shared encoding they still need. See
[demand-based publication](moq.md#experimental-demand-based-publication).

The existing outputs API includes `runtime_status` and `runtime_error` per
instance. The editor displays these separately from persisted settings. A
process-local `--output null` or another CLI output override replaces the whole
stored collection for that run without changing the database.
The CLI chooses the first active output of the requested mode in stored order,
then falls back to the first configured output and finally a legacy preset.
The selected output is enabled for this run even if it is disabled in settings.
Shared encoding sources and HLS variants are resolved before the other outputs
are removed, so the selected destination runs independently with its effective
encoding settings. `--output null` uses the primary output's effective encoding.

An enabled MoQ output is displayed first in the output editor and supplies the
program preview in the main Player window. Without an enabled MoQ output, the
configured HLS preview remains in use. Saving MoQ activation starts or stops its
delivery and importer without restarting the channel.

### Live output changes

Disabling a destination stops its delivery worker. Its encoder remains alive if
another output or packet-copy recording needs it. A configured encoding owner can
therefore be disabled as a destination while still supplying media to its consumers.
When the last consumer is removed, its encoding group is stopped. All destinations
can be disabled while the shared program timeline continues.

Enabling a destination uses the current program position and waits for a keyframe;
it does not replay media from the period when it was disabled. Changes to destination
URLs, muxer options or metadata replace only that delivery worker. Changing an
owner's encoding settings recreates that encoding group and its dependent deliveries;
unrelated groups continue. Adding HLS to a shared group can also recreate that
encoder if the new segment interval needs more frequent keyframes. Removing a
destination retains a compatible shorter keyframe interval. Closing an old connection is isolated from program pacing,
and replacements of the same destination wait for the old writer to close.

Program resolution/frame rate, desktop window activation/settings and recording
configuration still require a channel restart. Encoder changes affecting the recording
owner also require a restart to keep the active recording coherent. Changes combined
with any restart-required setting are deferred together until that restart. A
desktop runtime keeps an empty parallel output branch available, so its first
encoded output, including demand-based private MoQ, can start without a restart.
The empty branch creates no encoder or delivery workers. Desktop is selectable
only when the backend includes `desktop` or `desktop-cpu`.
Live output changes remain experimental and need sustained server validation.

### Choosing an encoding source

MoQ, Stream and HLS each have one **Encoding source** selector; there is no separate
sharing checkbox. **Own encoding** shows resolution, codec, bitrate and encoder
options. Selecting another output hides these private settings and retains them
for switching back. For HLS sources, select the named rendition as well. Save a
new output before using it as a source. The UI offers outputs with their own
encoding; select the original owner directly. Existing source chains remain readable.

MoQ packet reuse requires H.264/H.265 video and AAC audio. Other outputs follow
the codecs supported by their muxer. Advanced transport, muxer and metadata
settings remain independently editable. Recording can copy a derived output;
its HLS source selector offers the derived output's single playlist.

Output settings stay expanded; advanced options are collapsible and initially
open when custom options are configured. Runtime status refreshes every five
seconds while the Outputs page is open. Missing status information is shown as
unknown. MoQ status comes from its media importer.

Existing migration 0007 stores the source output ID and optional HLS variant.
The former `share_encoding` column is retained for reading old configurations;
it is synchronized from the selection on save and is no longer a user setting.
