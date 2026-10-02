# RTMP ingest diagnostics

Run against an enabled live listener on a test channel. The stream switches
the channel to live input. The script requires Python 3 and FFmpeg with
libx264 and AAC; it does not start or reconfigure ffplayout.

The source is encoded **before** publishing begins. By default it contains
1080p25 test video and a continuous 440 Hz tone, H.264 without B-frames,
a two-second GOP, and stereo AAC at 48 kHz. Delivery timing changes, while
media timestamps remain continuous. No automatic reconnect hides a failure.

From the repository root, replace the URL with your own ingest URL:

```sh
# Baseline: continuous, realtime delivery.
python3 scripts/test_rtmp_ingest.py rtmp://127.0.0.1:1936/live/stream \
  --scenario normal --log-dir /tmp/rtmp-normal

# A short delivery pause on the same connection; normally it should survive.
python3 scripts/test_rtmp_ingest.py rtmp://127.0.0.1:1936/live/stream \
  --scenario pause --pause-seconds 0.75 --log-dir /tmp/rtmp-short-pause

# A longer pause: the current 1.5-second watchdog may deliberately disconnect.
python3 scripts/test_rtmp_ingest.py rtmp://127.0.0.1:1936/live/stream \
  --scenario pause --pause-seconds 3 --log-dir /tmp/rtmp-long-pause

# Simulate a publisher draining a backlog: 15 source seconds at 1.5x,
# followed by normal pacing, on the same connection.
python3 scripts/test_rtmp_ingest.py rtmp://127.0.0.1:1936/live/stream \
  --scenario burst --burst-rate 1.5 --log-dir /tmp/rtmp-burst
```

Each directory must be new. Without `--log-dir`, a unique temporary directory
is created. The scenario starts at source second 10 (`--at`). `--duration`
counts source seconds: a burst shortens wall time, a pause lengthens it.
`--input /path/to/video.mp4` uses your own video/audio instead of the pattern.
`--source-flv /tmp/rtmp-normal/source.flv` reuses an already prepared source;
it must be long enough and have clean H.264/AAC timestamps. This option avoids
encoding again. `--size 1280x720` reduces the default test resolution.

## Evidence to collect

Keep ffplayout's debug logs covering the complete run and listen to/watch its
output. Publisher success alone does not prove the output was continuous.

- `events.jsonl`: elapsed times, pause/burst boundaries, source progress,
  blocked writes to the publishing process, and its exit status. These
  counters measure delivery **to FFmpeg**, not receipt by ffplayout.
- `publisher.log`: FFmpeg diagnostics, including `Broken pipe` or a rejected
  connection. It can contain the ingest URL/key; redact before sharing.
- `progress.log`: FFmpeg output progress with elapsed wall times. It measures
  publisher progress, not successful decoding at the receiving side.
- `prepare.log`: source encoding diagnostics, if a source was generated.

For a listener on local port 1936, observe its TCP queue independently in
another terminal. This works without attaching to ffplayout's process name:

```sh
while true; do
  date -Ins
  ss -tn '( sport = :1936 )'
  sleep 1
done | tee /tmp/rtmp-ingest-sockets.log
```

Correlate `disconnected or idle`, queue-full warnings, publisher errors, and
the socket samples with scenario boundaries. A long intentional pause tests
the idle detector; it does not reproduce a false timeout with an active sender.
A short pause can be masked by receiver buffering, so repeat it after any
backlog has drained. Repeat bursts with `--burst-rate 2 --burst-seconds 30`
and a sufficiently long `--duration` if the first burst produces no queue
pressure. An actual output stall is a separate test: only block a dedicated
test receiver, then verify that backpressure does not terminate live ingest.

The script stops after a publisher failure or its wall-time limit
(`--timeout`, default source duration + pause duration + 30 seconds).
A successful run exits 0; a publisher/source failure exits nonzero, and a
timeout exits 124. A disconnect in the long-pause scenario can be expected;
the normal and burst scenarios should not disconnect merely due to queue
pressure. These tests cannot by themselves rule out an intermittent fault
that appears only after hours or days.

## Local verification

Checked against the engine's `playout` example with real localhost RTMP ingest
and output, using a 1080p25 source:

| Scenario | Result |
| --- | --- |
| Realtime baseline, 12 source seconds | No disconnect |
| 750 ms delivery pause, 20 source seconds | No disconnect |
| 3 second delivery pause | Watchdog disconnect, publisher reports broken pipe |
| Requested 2x burst for 25 source seconds, 40 seconds total | No disconnect; publisher writes were throttled by backpressure |
| Output receiver blocked for 12 seconds | Queue-full warnings for several seconds, no watchdog disconnect; recovered after release |
| Output receiver blocked for 20 seconds | Output write timeout; no live-watchdog disconnect |

No audio silence was detected in the recorded baseline, burst or recovered
12-second output-stall runs. The existing unit test
`backpressure_does_not_make_the_live_watchdog_abort_the_reader` also passed.
The false idle teardown reported in issue #986 was not reproduced by these
runs. A requested burst rate is a scheduling target: blocked writes can reduce
the actual rate, which is visible in the progress and event logs.
