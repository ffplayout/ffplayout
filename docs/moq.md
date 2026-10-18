# MoQ output and browser preview

MoQ is included in every application build; no additional Cargo feature is needed.
MoQ consumes the common output pipeline through a local MPEG-TS connection. Start the application with
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
advertises available FFmpeg encoders supported by the current importer, including
supported hardware encoders; validation uses codec IDs rather than names such as
`libx264` or `libx265`.

The current MPEG-TS-to-MoQ bridge supports H.264/H.265 video with AAC audio for this
application. AV1 is not advertised because the pinned `moq-mux` MPEG-TS importer
has no AV1 video import path. Enabling AV1 requires a compatible import/container
path and browser playback validation; changing a SQL constraint cannot add that
support. Extending transport capabilities does not require a database migration.

Browser codec support, GOP length, B-frames and encoder buffering affect playback
and latency. An enabled output encodes or forwards media even when no viewer is
connected. It does not start another playlist player or ingest listener.

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

The Outputs page refreshes runtime status every five seconds. MoQ importer state
reflects received media; missing status information is shown as unknown.

Use `--log-level trace --log-to-console` for timing summaries. The hidden
`--timing-diagnostics` flag (or
`FFPLAYOUT_TIMING_DIAGNOSTICS=true`) enables additional primary video
measurements without enabling a MoQ output. Trace-level timing collection is
otherwise gated to avoid unnecessary diagnostic work.

`[Output Timing] stage=mux` reports per-track gaps, drift, queue residence and mux
call duration. `[Preview Timing] stage=importer` reports TCP reads, bytes, maximum
read gap and import call time. Compare several windows and their timestamps:
TCP read boundaries are not frame boundaries, and successful mux calls do not
prove browser presentation timing.

The relay cache is bounded to 32 MiB with two seconds of retention. Destination
I/O and packet queues are independent; a failed MoQ transport does not stop other
outputs using its encoding profile. Failed destinations retry every 15 seconds for
up to three minutes; successful media delivery resets that window. After exhaustion,
use **Reconnect** on the Outputs page to start a new round. Importer initialization
is retried on the same schedule, and a terminated importer is recreated on the next
connection attempt. Encoder failures still require a channel restart.
Endurance tests with browser audio/video
and clip transitions are still needed for the current multi-output implementation.
