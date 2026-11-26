# FFmpeg versus native HLS packager

Initial measurements on 2026-11-11, Apple M5 Pro, 48 GiB RAM, macOS arm64,
local APFS storage. This compares the two current ffplayout output paths,
including their configured muxer options, rather than every possible FFmpeg setup.

## Method

Each backend receives the **same encoded packets** from a ten-second MP4 fixture:
H.264 at 1920×1080 / 4.5 Mbit/s, 1280×720 / 2.4 Mbit/s and 854×480 /
1.2 Mbit/s, 25 fps, closed GOPs every two seconds, two B-frames, AAC at
48 kHz / 128 kbit/s per rendition. The source is repeated with continuous PTS/DTS.
Each submitted packet receives its own payload allocation, so fixture reuse does
not hide the native segment buffer's memory use.

Encoding and input demuxing happen outside the timed region. Measurements include
packet allocation, the engine's synchronous packet-delivery path, TS muxing,
WebVTT generation, playlist publication and file cleanup. This is the path used
by shared encoded HLS destinations; it does not simulate all playout workers,
decoding, graphics, network clients or real-time input pacing.

Each configuration runs three times, in a fresh process and fresh output directory.
Backend order alternates. Tables show medians. CPU means user + system CPU time;
RSS is the process peak, including fixture data and the test harness. Resume time
is measured separately after finalizing the output. The filesystem cache is warm,
and no extra fsync is added. These are accelerated throughput measurements,
**not an hour-long real-time soak test or a prediction of total playout CPU/RAM**.

Settings: 6-second segments, `hls_delete_threshold=1`, no program-date-time flag,
three visible segments for short windows, 600 for the fully populated hour.
WebVTT has one three-second cue every ten seconds, including segment crossings.

[Raw measurements](hls-packager-comparison.csv) contain all 30 process runs.
These measurements precede the short-window retention correction: native now
keeps at least three TARGETDURATIONs (24 seconds for a target of 8), even if
three entries were requested. The short-window measurements therefore describe
the earlier count-only behavior and need remeasurement for the current path.
The 600-entry hour windows already exceed that minimum.

## CPU and process memory

| Scenario | Media processed | FFmpeg CPU | Native CPU | Native reduction | FFmpeg peak RSS | Native peak RSS |
|---|---:|---:|---:|---:|---:|---:|
| One 1080p rendition, short window | 120 s | 30.8 ms | 25.6 ms | 17% | 103.5 MiB | 39.3 MiB |
| Three renditions, short window | 120 s | 74.3 ms | 68.2 ms | 8% | 111.0 MiB | 49.0 MiB |
| Three renditions + WebVTT, short window | 120 s | 70.2 ms | 63.8 ms | 9% | 113.2 MiB | 48.9 MiB |
| Three renditions, full 60-minute window | 3600 s | 1798.0 ms | 1546.6 ms | 14% | 263.5 MiB | 49.0 MiB |
| Three renditions + WebVTT, full 60-minute window | 3600 s | 1885.0 ms | 1662.0 ms | 12% | 266.0 MiB | 49.0 MiB |

The native path uses less CPU and reaches a lower process peak in this test.
The short runs are only tens of milliseconds, so small percentage differences
should not be overinterpreted. The large-window memory difference warrants a
Linux/RAM-disk and real-time confirmation; peak RSS does not establish a leak.
The native compressed segment buffer is still bounded at 64 MiB / 32768 packets,
and supported long segments or high aggregate bitrates can reach that bound.

## Throughput, resume and storage

| Scenario | FFmpeg wall time | Native wall time | FFmpeg resume | Native resume |
|---|---:|---:|---:|---:|
| One 1080p rendition, short window | 49.0 ms | 43.2 ms | 0.34 ms | 0.22 ms |
| Three renditions, short window | 98.2 ms | 84.9 ms | 0.83 ms | 0.42 ms |
| Three renditions + WebVTT, short window | 102.9 ms | 98.6 ms | 0.80 ms | 0.51 ms |
| Three renditions, full 60-minute window | 1806.8 ms | 1588.9 ms | 7.47 ms | 6.42 ms |
| Three renditions + WebVTT, full 60-minute window | 1886.1 ms | 1695.4 ms | 9.13 ms | 9.71 ms |

With the full hour and WebVTT, the resulting directories contain approximately
3.989 GB for FFmpeg and 4.025 GB for native: native is about 0.9% larger.
Both contain 600 video segments per rendition and a 600-entry subtitle window.
Storage differences in short-window raw results also reflect resume cleanup:
the existing FFmpeg resume path prunes unreferenced segments, whereas native
preserves its configured one-segment reserve. The byte counts were taken after
that separate resume measurement, and therefore are not a steady-state retention
comparison for the short windows.

## Output checks and functional differences

- For the full hour, every rendition contains **90000 video packets and 168840
  AAC packets** with either backend. The short visible windows contain the
  expected 450 video packets per rendition. Audio counts at short-window edges
  differ because the two muxers partition audio around segment boundaries.
- Every retained segment reference exists. Sampled segment starts decode
  independently, and packet counting across every retained video segment succeeds
  without ffprobe errors. This checks packet coverage and sampled decoding;
  it is not a perceptual playback evaluation.
- Native writes `X-TIMESTAMP-MAP`. For the segment beginning at source second
  102, its WebVTT map has LOCAL 102 s / MPEGTS 9180000 and the first video PTS
  is exactly 102 s. The FFmpeg output has no map, with first video PTS 102.080 s
  in this fixture/configuration. A/V timestamps receive the same initial FFmpeg
  shift; this observation alone does not show an A/V synchronization defect.
- The cue from 100–103 s is repeated into native's segment beginning at 102 s.
  In the FFmpeg output, that segment's VTT is empty; the cue exists only in the
  preceding VTT segment. Native explicitly retains cues across segment boundaries.
- With six-second segments, **native declares TARGETDURATION 8; FFmpeg declares 6**.
  Native reserves two seconds for the shared keyframe grid. Actual complete
  segments in this fixture are six seconds in both outputs. Player live-edge
  selection can depend on TARGETDURATION; a browser test is needed before claiming
  equal end-to-end live latency. First playlist publication in the packet stream
  occurs around media second six for both.
- Existing engine regression tests cover fresh starts when switching backends
  in either direction, same-backend resume, restarting delivery without restarting
  shared encoders, re-enabled subtitles and different initial scene-cut keyframes.

Native remains experimental. Its initial supported scope is local MPEG-TS with
H.264/AAC and optional WebVTT; most custom FFmpeg muxer options are rejected.
Full browser/player qualification, a long real-time soak, Linux engine measurements
and large-bitrate buffer behavior remain required before making it the default.

## Boundary and platform regressions

- With keyframe flags at 6 and 18 seconds withheld, both backends produce
  **8, 4, 8, 4, 6 seconds** and retain all 750 video / 1407 AAC packets per
  rendition over 30 seconds. No ffprobe errors occurred in this comparison.
- Container-independent tests start at 1.6 seconds and verify a 6.4-second first
  segment followed by six-second segments. Further tests cover different
  rendition keyframes, every packet's identity, captions crossing a delayed cut,
  and missing keyframes. Nominal targets remain relative to the initial common
  keyframe, so a late cut does not shift every following target.
- If a segment would exceed the fixed TARGETDURATION, publication fails before
  consuming its packets or writing media files; the buffer and wait limits still
  apply. No media is trimmed to make the playlist fit.
- All **17 ff-hls tests** also pass on aarch64 Linux in a Debian Lima VM with
  temporary files on `/dev/shm`. This verifies the FFmpeg-free core on a RAM-disk;
  it is not a Linux engine performance measurement.
- Both backends completed a three-minute paced run with three renditions and
  captions, including delayed boundary flags. A Safari native-HLS smoke test
  played video and displayed captions from the native output without a media
  error. It was a short late-entry test; playback stalled after the finite test
  writer stopped. Variant switching, seeking, other players, end-to-end latency
  and a 24-hour soak remain unqualified.

## Remeasurement on 2026-11-26

Revision `49b355fd` plus the benchmark options below, macOS arm64 on local APFS,
FFmpeg build `47b92841e3` (`libavformat 63.12.100`, `libavcodec 63.22.100`).
The harness submitted the same H.264/AAC fixture to both backends with six-second
segments. Each row shows the median of three fresh processes per available backend.
The 48 runs were accelerated and excluded decoding and encoding.

| Scenario | FFmpeg CPU | Native CPU | FFmpeg peak RSS | Native peak RSS |
|---|---:|---:|---:|---:|
| TS, one variant, short window | 29.1 ms | 24.7 ms | 103.3 MiB | 39.2 MiB |
| TS, three variants, short window | 62.7 ms | 54.3 ms | 110.9 MiB | 48.7 MiB |
| TS, three variants, hour window | 1.801 s | 1.537 s | 263.2 MiB | 48.8 MiB |
| TS, three variants + WebVTT, short window | 68.1 ms | 59.7 ms | 112.9 MiB | 48.3 MiB |
| TS, three variants + WebVTT, hour window | 1.898 s | 1.650 s | 265.6 MiB | 48.9 MiB |
| TS, three variants + WebVTT + PDT, short window | 93.2 ms | 59.0 ms | 113.2 MiB | 48.9 MiB |
| TS, three variants + WebVTT + PDT, hour window | 79.496 s | 1.641 s | 266.0 MiB | 49.7 MiB |
| fMP4, three variants + WebVTT + PDT, short window | — | 112.4 ms | — | 77.7 MiB |
| fMP4, three variants + WebVTT + PDT, hour window | — | 3.265 s | — | 261.0 MiB |

The FFmpeg HLS backend in ffplayout supports MPEG-TS for this comparison.
On the test host, optional `EXT-X-PROGRAM-DATE-TIME` (PDT) dominates CPU time
for its full window. In a separate ten-minute run, FFmpeg used 2.51 s CPU with
the system timezone and no `TZ` override, 2.03 s with `TZ=Europe/Berlin`, and
1.41 s with `TZ=UTC`. These figures describe local playlist generation on this
system. FFmpeg formats the date for each entry when rewriting a playlist.
[FFmpeg HLS playlist source](https://ffmpeg.org/doxygen/trunk/hlsplaylist_8c_source.html)

- Both backends published identical H.264/AAC codec declarations and aligned
  six-second segments. Full windows contained 90,000 video and 168,840 audio
  packets per variant. All referenced files existed. Sampled segments decoded
  independently, and a seek to 30 minutes decoded 25 frames in each rendition
  of each full window using FFmpeg as a reader.
- In the MPEG-TS samples, identical payloads had a common 80 ms PTS shift in
  the FFmpeg backend, for video and audio alike. Native fMP4's sampled first
  audio and video PTS differed by at most 10.7 ms.
- Native generated matching program dates on all three variants and WebVTT,
  wrote `X-TIMESTAMP-MAP`, and included the caption spanning 100–103 s in
  both adjacent VTT segments. FFmpeg's WebVTT playlist omitted PDT, and its VTT
  segment for 102 s omitted the crossing cue and the timestamp map.
- With three segments requested, both backends retained 18 s. A second
  retention check with two segments requested kept 18 s in native and 12 s in
  FFmpeg, in video and WebVTT alike. Both declared `TARGETDURATION:6`.
- The native fMP4 clock regression passed separately for H.264, HEVC and AV1.

The measurements cover packaging throughput, packet timestamps and local
media seeking. Deployment CPU/RSS, player latency and live rendition
switching depend on the full playout and client paths.

## Reproduction

Create the six-track fixture (commands used for these measurements):

```sh
mkdir -p /tmp/hls-compare
ffmpeg -y -f lavfi -i testsrc2=size=1920x1080:rate=25 \
  -f lavfi -i sine=frequency=997:sample_rate=48000 \
  -filter_complex '[0:v]split=3[a][b][c];[b]scale=1280:720[b1];[c]scale=854:480[c1]' \
  -map '[a]' -map 1:a -map '[b1]' -map 1:a -map '[c1]' -map 1:a \
  -t 10 -c:v libx264 -preset veryfast \
  -b:v:0 4500k -b:v:1 2400k -b:v:2 1200k \
  -g 50 -keyint_min 50 -sc_threshold 0 -bf 2 \
  -threads:v:0 2 -threads:v:1 2 -threads:v:2 2 \
  -c:a aac -b:a 128k -avoid_negative_ts disabled /tmp/hls-compare/source.mp4
```

MP4 preserves the initial negative video DTS for this fixture. The benchmark expects a ten-second source with alternating
video/audio tracks, as created above. Each destination directory must be new.
Run one backend per process, alternating order and repeating three times:

```sh
HLS_BENCH_SOURCE=/tmp/hls-compare/source.mp4 \
HLS_BENCH_DIRECTORY=/tmp/hls-compare/native-hour-1 \
HLS_BENCH_BACKEND=native HLS_BENCH_VARIANTS=3 \
HLS_BENCH_SECONDS=3600 HLS_BENCH_SEGMENT_SECONDS=6 \
HLS_BENCH_LIST_SIZE=600 HLS_BENCH_SUBTITLES=1 \
cargo test -p ff-engine --release --no-default-features --lib \
  compare_hls_packagers -- --ignored --nocapture --test-threads=1
```

Set the backend to `ffmpeg` for the counterpart, `VARIANTS=1` for the single
rendition, `SUBTITLES=0` to omit captions, and `SECONDS=120` / `LIST_SIZE=3`
for the short window. The test emits one `HLS_BENCH` JSON record. The current
`reopen_ms` field measures opening the output after finalization. Native HLS
decides whether program dates overlap when it receives the first common
keyframe, after that measurement. Verify the
resulting media outside the timed region; append `#EXT-X-ENDLIST` to a copy of
a media playlist before using ffprobe packet counting, so it does not poll a
finished live playlist. Remove generated directories after inspection.

Additional controls: `HLS_BENCH_REALTIME=1` paces submission by DTS;
`HLS_BENCH_SKIP_KEYFRAME_MS=6000,18000` suppresses only boundary flags at those
absolute PTS values while retaining the encoded payload; `HLS_BENCH_START_MS`
skips earlier packets for exploratory midstream-entry tests. These controls are
not enabled in the 30-run throughput table.

Set `HLS_BENCH_PROGRAM_DATE_TIME=1` to measure publication with wall-clock
tags. Set `HLS_BENCH_SEGMENT_TYPE=fmp4` with the native backend to measure
fragmented MP4. The ffplayout FFmpeg backend uses MPEG-TS for this comparison.
