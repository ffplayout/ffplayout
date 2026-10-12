# Live-input memory tests

These opt-in tests exercise the listener-restart pattern reported in
[issue #986](https://github.com/ffplayout/ffplayout/issues/986#issuecomment-5977499282).
They require Linux or macOS, `ffmpeg` with libx264/AAC on `PATH`, `ps`, `kill`,
and permission to open local TCP sockets. FFmpeg libraries linked into the
engine must support RTMP, H.264 and AAC.

Run each test in its own process. RSS covers the entire test process, so other
tests must not run alongside it. The tests are ignored by ordinary `cargo test`
and do not increase normal CI runtime.

```sh
cargo test -p ff-engine --no-default-features --features tokio --lib \
  input::tests::memory::rtmp_memory_continuous_stream \
  -- --exact --ignored --nocapture --test-threads=1

cargo test -p ff-engine --no-default-features --features tokio --lib \
  input::tests::memory::rtmp_memory_reconnects \
  -- --exact --ignored --nocapture --test-threads=1
```

## Scenarios

- **Continuous:** one RTMP publisher remains connected throughout all measurement
  windows. The test checks continued audio/video delivery and rejects unexpected
  session changes. This provides a comparison for growth during steady streaming.
- **Reconnects:** publishers repeatedly connect to the same listener, while the
  output encoder remains open. Odd cycles terminate the publisher; even cycles
  pause its process with `SIGSTOP` until decoded media has stopped for longer
  than the configured live idle timeout. The publisher then receives `SIGCONT`,
  allowing blocked FFmpeg reads to return and the watchdog teardown to complete.
  Even cycles publish at 1.5x to exercise recovery bursts and queue backpressure.
  Each cycle checks audio delivery, session advancement, pending-audio cleanup,
  and eventual reader cleanup before measuring the new baseline.

The test generates a four-second looping 1920x1080/25 fps H.264/AAC fixture,
publishes it with `ffmpeg -c copy`, and runs the actual ingest decoder, frame
queue, live timeline handling, and a persistent libx264 `veryfast`/AAC output
encoder. The output uses MPEG-TS written to `/dev/null`, avoiding accumulating
output files. The consumer yields between frames to allow regular measurements
even when its queue stays full; it does not reset the live session or encoder.

This isolates the engine's live-input lifecycle. It does **not** reproduce the
Docker bridge, datarhei Restreamer, outgoing RTMP connection, application playlist
and filler handling, or a complete service restart. It can expose resource growth
on reconnects without guaranteeing reproduction of the reported 185 MB jump.

## Measurements and configuration

Each run prints its artifact directory under `target/live-memory/` and keeps:

- `memory.csv`: elapsed time, cycle, phase, resident bytes, Linux thread count,
  reader permits, stuck readers, session ID, and encoded audio/video frame counts.
  Samples are taken approximately every 500 ms; a blocking codec operation can
  delay them. A listening attempt itself holds one reader permit.
- `publisher-N.log`: FFmpeg publisher diagnostics per connection.
- `publisher.flv`: generated input, reusable for manual reproduction.

The final assertion compares the largest checkpoint RSS after warmup with the
checkpoint at the end of warmup. Reconnect checkpoints follow teardown, reader
cleanup, and one second of settling; continuous checkpoints measure the ongoing
stream. All warmup samples remain in the CSV, so early jumps are also visible.

| Environment variable | Default | Meaning |
| --- | --- | --- |
| `FFPLAYOUT_MEMORY_WARMUP` | `3` | Initial cycles excluded from the RSS assertion |
| `FFPLAYOUT_MEMORY_CYCLES` | `8` | Measured cycles after warmup |
| `FFPLAYOUT_MEMORY_SESSION_SECONDS` | `3` | Minimum decoded video duration per cycle |
| `FFPLAYOUT_MEMORY_WIDTH` | `1920` | Input/output width, positive and even |
| `FFPLAYOUT_MEMORY_HEIGHT` | `1080` | Input/output height, positive and even |
| `FFPLAYOUT_MEMORY_MAX_GROWTH_MB` | `64` | Allowed checkpoint growth in MiB after warmup |
| `FFPLAYOUT_MEMORY_REPORT_DIR` | `target/live-memory` | Parent directory for run artifacts |

Numeric settings must be positive integers. These standalone engine tests use
the default two-second live idle timeout; application listener settings are
configured separately in the frontend.

For a longer reconnect run:

```sh
FFPLAYOUT_MEMORY_CYCLES=100 FFPLAYOUT_MEMORY_SESSION_SECONDS=10 \
  cargo test -p ff-engine --no-default-features --features tokio --lib \
  input::tests::memory::rtmp_memory_reconnects \
  -- --exact --ignored --nocapture --test-threads=1
```

The 64 MiB budget is a diagnostic threshold, not a proof of a leak or an expected
production memory limit. Codec pools and the allocator may keep freed memory
resident. Compare continuous and reconnect traces, active streaming peaks and
post-teardown checkpoints, and repeat under the affected Linux/FFmpeg environment
before attributing a persistent step to unreleased resources.
