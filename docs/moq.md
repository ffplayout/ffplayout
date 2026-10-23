# MoQ output and browser preview

MoQ is included in every application build; no additional Cargo feature is needed.
MoQ receives encoded frames directly from the common output pipeline through the
`moq-mux` track API. Start the application with
`--listen` to make its media relay and HTTP endpoints available.

## Configuration

Under **Settings → Outputs**, add a **MoQ** output and choose its encoding source:

- **Own encoding** exposes resolution, codec and encoder options.
- **Stream, HLS or another compatible output** reuses that output's encoded packets.
- **HLS variant** selects the base rendition or a named additional variant.

The same source selection is available for Stream and HLS outputs. Sources must
be saved and have their own encoding. A source destination can be disabled while
its encoder continues serving consumers. Cycles and incompatible codec/container combinations are
rejected. Selecting a source hides private encoding settings; transport, muxer
and metadata options remain independent. Saving MoQ changes updates its running output without a channel restart.
See [output instances](output.md#multiple-output-instances) for queues and failure handling.

MoQ settings are stored as ordinary `config_output` rows with `name = 'moq'`.
There is no `config_preview` or `config_moq` table and no separate preview
configuration. The MoQ output also supplies the main Player window's preview.

## Codecs

The database stores encoder names without a codec whitelist. The codec API
advertises available FFmpeg encoders validated for the native publisher, including
supported hardware encoders; validation uses codec IDs rather than names such as
`libx264` or `libx265`.

The currently validated combinations are H.264/H.265 video with AAC audio.
The direct API can support additional codecs, but they are not advertised until
their encoder initialization and browser playback have been validated.
Extending transport capabilities does not require a database migration.

Browser codec support, GOP length, B-frames and encoder buffering affect playback
and latency. MoQ does not start another playlist player or ingest listener.
An independent MoQ output using `libx264` always uses `tune=zerolatency`; the Tune
selector is omitted for that encoder. When MoQ reuses another output's encoding,
it keeps that source's settings, including lookahead and B-frames. Such profiles
add startup/playback latency. Encoded delivery queues are bounded by packet count
and bytes rather than an audio/video timestamp span, so encoder delay alone does
not cause a queue-capacity failure. A genuinely blocked destination can still
exhaust those bounds and reconnect independently.

## Experimental demand-based publication

An enabled private MoQ output starts when the internal Player opens its MoQ
preview. It stays active while at least one authenticated relay connection exists,
including paused players and multiple browser tabs. When the last connection
ends, publication continues for three minutes; reconnecting cancels that deadline.
An authorized player bootstrap reserves up to 60 seconds for the handshake. If no
connection follows, that temporary reservation expires automatically.

With **Allow public player** enabled, the MoQ output publishes continuously even
without viewers. Disabling the output always stops publication. These runtime
transitions do not change saved settings or restart the channel.

When private publication sleeps, its packet delivery and meter publisher stop. Its own
encoder also stops if no other output or recording needs it; shared HLS/Stream
encoders and their outputs continue. The relay stays reachable. Opening the
player again incurs encoder/publisher startup and a wait for a usable keyframe;
the delay depends on the encoder and GOP length. No extra frontend heartbeat or
periodic HTTP request is required. The Outputs page reports the sleeping output
as not running.

## Player and transport

The internal Player page embeds the shared Vue MoQ component directly. It uses
`/api/moq/config?channel=ID` with the existing login and channel permissions.
The component and official MoQ player are bundled with the frontend; playback
needs no CDN imports. It starts paused with audio enabled, with a 100 ms jitter buffer.
Codec and server buffers add to this delay; it is not an end-to-end latency guarantee.

**Allow public player** on a MoQ output is off by default. Saving this permission
applies immediately without restarting encoding. When enabled, the public Vue
page `/moq/player?channel=ID` uses the same component and can be opened without
login. Its bootstrap endpoint `/moq/config?channel=ID` is unavailable otherwise.

The relay checks every WebSocket/QUIC/WebTransport admission. Anonymous viewers
receive a subscription grant for exactly one enabled, public channel and cannot
publish or subscribe to other channels. Private viewers use single-use,
channel-bound handshake tickets valid for 60 seconds, issued after login. The
application access token stays on the server and is revalidated during the session.
Public opt-out or a disabled output revokes existing media sessions within five
seconds. Expired private login credentials also end the media session; the Vue
component obtains a fresh ticket through the normal token refresh flow.
Ticket renewal starts only after the MoQ library reports a terminal connection
error. Temporary bootstrap failures retry after 2, 4, 8, 16 and then at most every
30 seconds. Recovery, a changed connection URL or leaving the page cancels
pending attempts; HTTP 400, 401, 403 and 404 stop retries.

The same-origin WebSocket route is `/moq/relay?channel=ID`; direct relay URLs use
`/channels?channel=ID`. Both enforce the same authorization rules. Output configuration
and playout control APIs retain their existing authentication.

In Vite development mode, the player always uses the same-origin WebSocket proxy.
`/moq/config`, `/moq/relay` (including WebSocket upgrades), and `/api/moq/config`
are forwarded to the backend at `127.0.0.1:8787`. `/moq/player` stays on the Vue
dev server at `127.0.0.1:5757`. Direct relay URLs are used only in production builds.

For nginx, add the following location to the existing HTTPS server block, alongside
the regular frontend/API proxy. This example assumes ffplayout runs on the same
host at `127.0.0.1:8787`; adjust the upstream address for another host or container.
Leave `FFPLAYOUT_MOQ_PUBLIC_URL` unset to use the same-origin WebSocket route.

```nginx
location = /moq/relay {
    proxy_pass http://127.0.0.1:8787;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_buffering off;
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;
}
```

The proxy preserves the path and query parameters. With this setup, MoQ uses
the existing HTTPS port (normally TCP 443); the internal relay port 8089 does
not need public forwarding. See [nginx WebSocket proxying](https://nginx.org/en/docs/http/websocket.html).

For a deployed WebTransport endpoint, set `FFPLAYOUT_MOQ_PUBLIC_URL` to its public
MoQ URL ending in `/channels` and provide browser-trusted TLS and HTTP/3/WebTransport forwarding. Both Vue views use the player's fallback audio buffering when cross-origin
isolation is unavailable.

## Timing and status

Browser status and playback diagnostics are disabled by default. In Vite development
mode, open `/player?channel=ID&diagnostics=1` to opt in, then reload the page.
Production builds and the public player never run these measurements. Private
connection-ticket renewal remains active independently of diagnostics.

The Outputs page refreshes runtime status every five seconds. MoQ publisher state
reflects received media; missing status information is shown as unknown.

Use `--log-level trace --log-to-console` for timing summaries. The hidden
`--timing-diagnostics` flag (or
`FFPLAYOUT_TIMING_DIAGNOSTICS=true`) enables additional primary video
measurements without enabling a MoQ output. Trace-level timing collection is
otherwise gated to avoid unnecessary diagnostic work.

`[Output Timing] stage=mux` reports per-track gaps, drift, queue residence and mux
call duration (or native frame publication duration for MoQ). The old TCP-importer
timing stage no longer exists. Successful output writes do not prove browser
presentation timing.

The relay cache is bounded to 32 MiB with two seconds of retention. Destination
I/O and packet queues are independent; a failed MoQ transport does not stop other
outputs using its encoding profile. Failed destinations retry every 15 seconds for
up to three minutes; successful media delivery resets that window. After exhaustion,
use **Reconnect** on the Outputs page to start a new round. Publisher initialization
is retried on the same schedule, and a terminated publisher is recreated on the next
connection attempt. Encoder failures still require a channel restart.
Endurance tests with browser audio/video
and clip transitions are still needed for the current multi-output implementation.

The Player page receives program peak/RMS levels through the existing MoQ
connection on the `audio-level.json` track, with 250 ms measurement and publication
intervals. Each reading carries the end timestamp of its measured audio window.
The publisher uses one shared decode-time anchor for audio and video, preserving
their relative presentation timestamps, encoder priming and video reordering.
The meter uses the exact audio-to-broadcast offset from the published AAC frames;
each connection starts with a fresh mapping.
The Player meter follows the audio worklet's read position and the browser's
reported output latency, aligning each 250 ms measurement window to its midpoint
to reduce visual lag. While paused, without audio playback, or after the playhead
has stopped advancing for 500 ms, the meter shows current program measurements.
Missing current measurements expire after one second; reconnects discard old
readings. Muting stops audio downloads in the player library, so the meter follows
the displayed video while muted and advancing.
The browser keeps at most 40 pending readings (about ten seconds), checks the
play position every 50 ms, and never drives or blocks media playback. This is a
250 ms windowed program meter, not a sample-accurate peak display; timing precision
also depends on the browser's playback-position and output-latency reports.
Browser volume does not change the measured program level. HLS previews continue
to show the current program level via SSE, independently of HLS playback delay.
Only authenticated Player and Audio configuration pages subscribe to this track; the public player
does not request meter data. An auxiliary publisher task keeps only
the latest reading, without an additional HTTP polling connection.

Audio configuration also uses MoQ for its peak/RMS and loudness meters when an
MoQ output is enabled. It subscribes to `audio-level.json` and
`audio-loudness.json` at 250 ms intervals without requesting media tracks or
starting a browser decoder. These configuration meters show current program
measurements immediately rather than following a playback position. Loudness analysis is enabled only while its track
has subscribers and released when they disconnect. Channels without an enabled
MoQ output retain their SSE meters. With MoQ enabled, meters use MoQ exclusively;
missing readings or a disconnected transport clear the display without switching
to SSE. During the initial
three-second short-term measurement window, the loudness bar shows the available
momentary value and labels it accordingly.
