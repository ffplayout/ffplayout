## Build ffplayout

For compiling, use the latest stable Rust version from [rustup](https://rustup.rs/).

### FFmpeg libraries

ffplayout links against FFmpeg through `ffmpeg-next`, so local builds need the FFmpeg runtime tools and development libraries. FFmpeg 7.0+ is required.

On Debian/Ubuntu based systems install:

```bash
sudo apt install \
    ffmpeg \
    libasound2-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswresample-dev \
    libswscale-dev \
    pkg-config
```

On macOS install:

```bash
brew install ffmpeg pkg-config
```

On Windows, download and extract the
[BtbN FFmpeg 9.0 GPL shared build](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip).
Point `ffmpeg-sys-next` to the extracted directory and make the DLLs available
while compiling:

```powershell
$env:FFMPEG_DIR = "C:\path\to\ffmpeg-n9.0-latest-win64-gpl-shared-9.0"
$env:PATH = "$env:FFMPEG_DIR\bin;$env:PATH"
cargo build
```

When compiling against a manually installed FFmpeg, make sure `pkg-config` can find the `.pc` files, for example:

```bash
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
```

### Optional SRT logging integration

The engine's build script uses `pkg-config` to check whether libsrt is linkable
for the target build. If `pkg-config --modversion srt` succeeds, ffplayout links
libsrt directly and routes its native messages through the ffplayout logger,
including rate limiting for repeated receive-buffer warnings. Use the
development files for the same libsrt installation as your FFmpeg build.

If libsrt or its `srt.pc` file is unavailable, ffplayout still builds and SRT
ingest can still work through an FFmpeg build that supports it. In that case,
libsrt's native messages may appear directly on stderr instead of in the
ffplayout logs. The provided static Docker build installs `srt.pc` alongside
libsrt, so its logger integration is enabled automatically.

`libavfilter` and `libavdevice` are optional and disabled by default to reduce
the runtime and static-build footprint. ffplayout currently uses its own media
processing pipeline, so FFmpeg filters such as `drawtext` and FFmpeg devices
such as DeckLink are unavailable in the normal build.

Enable them independently with the `ffmpeg-filter` or `ffmpeg-device` Cargo
feature. Local builds additionally need `libavfilter-dev` or `libavdevice-dev`.
The static-build script maps those features to FFmpeg automatically, for
example:

```bash
CARGO_FEATURES=desktop,embed_frontend,ffmpeg-filter,ffmpeg-device \\
    ./scripts/build.sh debian-static
```

To retain release debug symbols for CPU profiling, add `--profiling` to either
container build target:

```bash
CARGO_FEATURES=embed_frontend ./scripts/build.sh debian-shared --profiling
# Or: ./scripts/build.sh debian-static --profiling
```

This sets `CARGO_PROFILE_RELEASE_DEBUG=1` and
`CARGO_PROFILE_RELEASE_STRIP=false` in the build container. Release optimizations
remain enabled, and debug symbols are retained in both the binary and Debian
package. These two environment variables can also be passed directly or set in
`.env`; `--profiling` overrides their values. Build artifacts will be larger.

The build script keeps one stopped build container per workspace and target
(`ffplayout-build-debian-shared-…` / `ffplayout-build-debian-static-…`). Cargo and
npm download caches inside it survive subsequent builds. Containers are reused
regardless of age; a changed builder image requires recreation.
Features, `.env` values and profiling settings are
forwarded afresh on every build. Failed builds preserve the container/cache too.
An already running build container is left untouched and a concurrent build is
refused. Containers are stopped after use, so they consume disk space but no CPU.
An atomic lock directory under `target/.ffplayout-build-*.lock` protects the
container from inspection through shutdown. Normal exits, failures and handled
interrupts release it. After a forced kill or host crash, check that no build is
running before removing the leftover lock directory (its `pid` file records the
owning shell).
Image layers and the container engine's separate build cache are not pruned.
Build preparation prints timestamped phases and elapsed seconds for manual
generation, image preparation, metadata inspection, container creation/start,
build execution, and shutdown. If a build pauses before `npm ci`, the last
phase identifies the pending operation. Metadata inspection uses one image
request and one container request to reduce overhead from remote/VM CLI wrappers.

### Experimental transcription example

Whisper and ONNX Runtime are only enabled by the engine's optional
`transcription-example` feature. The normal ffplayout build does not include
them. To compile the desktop transcription example:

```sh
cargo build --locked -p ff-engine --example live_transcription --features transcription-example
```

Workspace checks with `--all-features --all-targets` also build this example.
On Apple Silicon, `.cargo/config.toml` registers a project-scoped CMake hook
for whisper.cpp. It checks native SVE compiler support before allowing GGML's
forced-SVE runtime probe, which can hang at its first SVE instruction on
[affected macOS systems](https://github.com/ggml-org/llama.cpp/pull/20064).
Other ARM feature checks, including DotProd, I8MM and SME, remain enabled.
The hook does nothing for other targets or CMake projects.

`ort` is pinned to `2.0.0-rc.12` for this feature because the currently used
`transcribe-rs` expects that API. Later ORT prereleases changed
`Session::builder`; review this pin when updating the transcription dependency.

## CPU profiling with perf (Linux)

Install your distribution's `perf` package matching the running kernel. Build
with `--profiling` as described above, install that build on the test server,
and start it with the normal playout configuration. For a local Cargo build,
retain symbols with:

```bash
CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=false \
    cargo build --release -p ffplayout
```

Use the same Cargo features as the build being investigated. Keep the exact
binary used for the recording: rebuilding or replacing it can prevent later
symbol resolution. Debug symbols for system libraries may also be needed to
resolve internal FFmpeg or codec functions in shared-library builds.

Find the running process and record 60 seconds of CPU samples, replacing `1234`
with its PID:

```bash
pgrep -a -x ffplayout
sudo perf record -F 99 --call-graph dwarf \
    -p 1234 -o ffplayout-cpu.data -- sleep 60
```

This attaches to the running process, including its threads, without restarting
it. Recording stops after 60 seconds; ffplayout continues running. Sampling and
stack collection add some overhead. `sudo` permits recording on systems with
restricted perf access without changing global kernel settings.

Open the interactive report or export it as text:

```bash
sudo perf report -i ffplayout-cpu.data
sudo perf report -i ffplayout-cpu.data --stdio > ffplayout-cpu-report.txt
```

Inspect function names and call chains to distinguish encoding, muxing, MoQ
importing, relay work and other application activity. Report percentages describe
the distribution of recorded CPU samples, not the process's overall CPU usage.
Inclusive call-chain percentages overlap and must not be added together. MoQ
and other asynchronous tasks share Tokio worker threads, so thread names alone
cannot reliably attribute CPU usage to MoQ.

For a MoQ comparison, record these three configurations separately, using the
same clip, HLS encoding settings and recording duration after startup settles:

- HLS alone.
- HLS plus MoQ using shared encoding, without a connected player.
- HLS plus MoQ using shared encoding, with a player actively playing.

Use distinct `.data` and report filenames for each case, and retain the matching
binary, build revision, configuration and process CPU measurements. The
`--process-diagnostics` option described below provides interval-averaged process
CPU usage alongside memory measurements.

See the upstream documentation for
[`perf record`](https://github.com/torvalds/linux/blob/master/tools/perf/Documentation/perf-record.txt)
and [`perf report`](https://github.com/torvalds/linux/blob/master/tools/perf/Documentation/perf-report.txt).

## Processing Benchmark

To compare per-encoder logo copies with resolution-shared composition, run:

```sh
cargo test -p ff-engine --release --lib benchmark_shared_logo_composition -- --ignored --nocapture
```

This manual benchmark compares 300 720p program frames with one, two and four
encoding groups at the same resolution. It reports median elapsed times after
warmup, alternates old/new order, and makes no timing assertion. It measures
frame copying/composition only, not codecs, network I/O or total playout CPU.

The standalone [4K frame-blend example](frame-blend.md) compares premultiplied
RGBA, linear RGB and YUVA composition, including SIMD, a fixed Rayon pool,
complete YUV420p pipeline timings, reference images and quality metrics:

```bash
cargo run --release -p ff-graphics --features ffmpeg --example frame_blend -- --samples 15
```

The optional `processing-bench` feature prints periodic wall-clock timings for
video and audio decode, audio processing and encoding, scale, overlay,
subtitle, encoded-output, and desktop presentation stages. It is intended for local CPU
profiling and is disabled in normal builds.

```bash
cargo run -p ffplayout --features processing-bench -- \
    --processing-bench-interval 5 \
    -l 127.0.0.1:8787 --log-to-console -o hls
```

`--processing-bench-interval` is measured in seconds and defaults to `1`.
The `FFPLAYOUT_PROCESSING_BENCH_INTERVAL` environment variable provides the
same setting. The reported times measure the instrumented caller thread; use
`perf` for CPU time inside FFmpeg worker threads.

## Hardware Encoding

The output codec list includes hardware encoders only when they accept the
engine's CPU-backed frames. NVENC accepts `yuv420p` directly. QSV receives a
CPU-side `nv12` frame produced immediately before the encoder and performs the
surface upload internally. VAAPI keeps decoding, scaling and overlays on the
CPU, then uploads the completed `nv12` frame to `/dev/dri/renderD128`
immediately before encoding. The process therefore needs access to the render
device, normally through the `render` group, in addition to a matching GPU
driver.

### Create Debian DEB and RHEL RPM Packages

Install the packaging tools:

- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

Build packages on the target distribution and architecture. `ffmpeg-next` links
against the system FFmpeg libraries, so cross-compilation also requires a target
sysroot containing matching FFmpeg development libraries and `pkg-config`
metadata. Rust's `--target` option alone is not sufficient.

For Debian-based systems:

```bash
cargo build --release -p ffplayout
cargo deb --no-build -p ffplayout
```

For RHEL-based systems, build the release binary first, then generate the RPM
from `crates/app`:

```bash
cargo build --release -p ffplayout
cd crates/app
cargo generate-rpm
```

For cross-compiled packages, provide a complete, compatible target FFmpeg
environment and verify the resulting binary on the target system.

## Generate types for Frontend
The frontend uses TypeScript, to generate types for the rust structs run: `cargo test`.

The generated types are written next to the frontend sources that import them.

## Automatic backups before database migrations

Before applying pending migrations to an existing file database, ffplayout creates
one consistent SQLite snapshot in the same directory. Committed data in the WAL
is included. Fresh databases, in-memory databases and starts without pending
migrations do not create backups.

Backups use the database filename, the local date, and the highest successfully applied
migration version **before** the update:

```text
ffplayout.db.2026-10-18.schema-6.bak
```

An existing database without migration history uses `schema-0`. A repeated
migration attempt with the same schema version and date replaces that day's
backup only after the new snapshot is complete. Backup creation or publication
failure aborts migration, preserving any previous backup at that path.

After a successful backup, ffplayout retains that snapshot and the four most
recently modified older backups matching this database's naming pattern. Other
files and manually named backups are left untouched. Cleanup failure also aborts
migration. The backup path and removed old backups are logged.

Restore a backup only while ffplayout is stopped, with the database's WAL and SHM
sidecars removed or moved aside. Keep a copy of the current database and its
sidecars before restoring, and use an application version compatible with the
restored schema.

### Roll back migration 7

Stop ffplayout before using the maintenance script. First validate the rollback
without committing changes or creating a backup:

```bash
python3 scripts/rollback_migration_7.py /path/to/ffplayout.db --dry-run
```

To return to schema 6:

```bash
python3 scripts/rollback_migration_7.py /path/to/ffplayout.db
```

This removes migration 7's output-instance settings and `program_format`, but
retains output rows and their older settings. Schema 6 permits only one active
output per channel and cannot represent shared encoding; incompatible active
configurations are rejected. Starting the current application reapplies migration 7.

To reset migration 7 and immediately apply the repository's SQL again while
preserving its settings, including multiple active outputs, use:

```bash
python3 scripts/rollback_migration_7.py /path/to/ffplayout.db --reapply --dry-run
python3 scripts/rollback_migration_7.py /path/to/ffplayout.db --reapply
```

Both changes occur in one transaction. Reapplication updates SQLx's migration
checksum. The script supports databases with successful migrations 1 through 7
and the migration 7 columns currently defined in this repository. It requires
Python 3 with SQLite 3.35 or newer. Before committing changes, it creates a
`<database>.<date>.schema-7.bak` snapshot and retains at most five schema backups
using the same filename pattern as the application. A failed rollback or
reapplication restores the transaction; its backup remains available.

## Database migrations on `main`

Database migrations added on the `main` branch are considered unstable until
the next ffplayout version containing them is released. Before that release,
an existing migration may still be changed, reordered, or combined with
another migration.

Running a self-built development version can therefore apply an intermediate
migration that is incompatible with a later commit. This can break the
migration chain or cause a migration checksum mismatch, and the affected
database may have to be reinitialized. Do not use builds from `main` with a
production database without first creating a restorable backup. Once a
migration has been included in a release, it must remain unchanged; subsequent
schema changes require a new migration.

## Setup Frontend

Make sure to install the dependencies:

```bash
# yarn
yarn install

# npm
npm install

# pnpm
pnpm install --shamefully-hoist
```

## Development Server

Start the development server on http://127.0.0.1:5757

```bash
npm run dev
```

## Production

Build the application for production:

```bash
npm run build
```

Check out the [deployment documentation](https://vuejs.org/guide/quick-start.html) for more information.

## Run ffplayout in development mode

1. run backend: `cargo run -- -l 127.0.0.1:8787`
2. in a second terminal:
    1. install packages: `npm ci`
    2. run frontend: `npm run dev`
3. in the browser navigate to: `http://127.0.0.1:5757`
4. Complete first-time setup in the frontend.

For an initial setup without the frontend, use `cargo run -- -i` instead.

### Process diagnostics

Use the hidden startup flag together with DEBUG logging:

```sh
ffplayout --process-diagnostics --log-level DEBUG
```

Alternatively, use the Clap environment variable:

```sh
FFPLAYOUT_PROCESS_DIAGNOSTICS=true ffplayout --log-level DEBUG
```

For systemd, add `Environment="FFPLAYOUT_PROCESS_DIAGNOSTICS=true"` to the service
configuration and restart the service. Both forms enable diagnostics for all
running channels. The setting defaults to false; an explicit CLI flag enables
it even if the environment variable is false. This setting is not part of the
TOML configuration or database and has no frontend field.

`[Process Diagnostics]` DEBUG records are produced immediately before opening each
non-skipped clip (after the preceding playback call has returned), and on actual
live takeover/end events. Additional `event=periodic` records are emitted every
five minutes, starting five minutes after the channel diagnostics worker starts.
The first clip establishes the RSS baseline. Clip numbers
count playback attempts, including filler, repeated sources and post-live attempts.
A single worker serializes measurements and logging; queued live transitions are
processed before the next clip boundary. Playback waits for its clip record.
Each live event requests a measurement asynchronously; this is not a snapshot
captured inside the decoder at the exact instant of the transition.

CPU usage is reported as `cpu_percent`, averaged over `cpu_window_s` seconds
since the preceding measurement for that channel. 100% means one fully occupied
CPU core; multithreaded usage can exceed 100%. The first measurement establishes
the CPU baseline and reports `unavailable`. CPU time includes all process threads,
including encoding, MoQ and the application, but excludes child processes.
Measurements are event-based and every five minutes; brief peaks may be averaged out.
Enable these measurements with `--process-diagnostics` or
`FFPLAYOUT_PROCESS_DIAGNOSTICS=true`.

Records contain `scope=process`, event, channel metadata, clip number, process
uptime in seconds, RSS bytes, signed RSS delta since the previous measurement for
that channel, PSS bytes on Linux (`pss_bytes`, `pss_anon_bytes`, and
`pss_file_bytes`), thread count where supported, live status,
listener ID and source kind (`playlist`, `folder`, `filler`, or `ingest`).
No media file paths are included in these diagnostic records. Live start uses
`ingest`; live end uses the preceding program source kind until the next clip
boundary establishes the newly selected source. Unavailable measurements are marked `unavailable`.
`pss_anon_bytes` represents anonymous memory, including heap and media buffers;
`pss_file_bytes` represents file-backed memory. Missing fields on older kernels
remain `unavailable` independently of the other fields.
Linux PSS is read from `/proc/self/smaps_rollup`. RSS/PSS cover the entire process,
so measurements from different channels cannot attribute growth to one channel.
The live end retains the listener ID of the ended session.

Periodic measurements use the same worker and RSS baseline as event measurements,
retain the current clip number and source, and report the current live status.
Queued live and clip events take priority; missed timer ticks are skipped instead
of producing catch-up measurements. Process inspection runs outside the
async executor on blocking workers, and no measurements run unless diagnostics
are enabled and DEBUG logging is active. Existing periodic development metrics
remain independent.

### RTMP/HLS recovery regression

For the opt-in end-to-end RTMP/HLS recovery test, run alone with a recent FFmpeg
CLI providing `-readrate_catchup`, `libx264`, and AAC, and permit local TCP sockets:

```bash
RUST_LOG=info cargo test -p ff-engine --lib \
  rtmp_reception_pauses_and_reconnects_feed_three_hls_variants -- --ignored --nocapture
```

The approximately 90-second run sends 720p H.264/AAC through the real RTMP
listener and decoder into three HLS variants and an independent stream encoder.
It pauses/resumes the publisher, requests fourfold catch-up, reconnects with
Full-Range video, injects a sender PTS jump, and changes resolution mid-stream.
Output AAC uses `libfdk_aac` when available. Logs and HLS artifacts remain in
`tmp/rtmp-hls-recovery-<pid>/`. Set `FFPLAYOUT_RTMP_CASE=0`, `1`, `2`, or `3`
to run only the normal, Full-Range, PTS-jump, or resolution-change case.
After draining the outputs, the test checks audio/video packet counts and
strictly increasing per-track DTS in all three HLS variants and the independent
stream, then decodes both tracks with FFmpeg in strict error mode. Finished HLS
test playlists receive an end marker so verification reaches EOF.

Set `FFPLAYOUT_RTMP_SERVER_PROFILE=1` for the recovery configuration with a
single owning HLS group (720p/480p/240p, CRF 23, faster, zerolatency,
libfdk_aac when available), six-second HLS segments, VTT, a fixture logo, and
program-wide loudness processing. A native packet sink shares the 480p rendition
through the same paced packet interface used by MoQ; an FLV file shares 720p.
This exercises encoding sharing and the native delivery boundary, but does not
start a MoQ relay or connect to an external RTMP server. Source pauses, reconnects,
format changes, and media verification remain the same.

Add `FFPLAYOUT_RTMP_SYNC_MARKERS=1` for an approximately 25-second run with
simultaneous white flashes and audio tones at irregular intervals. It removes
1.8 seconds of video while audio continues, and also pauses/resumes reception.
Decoded flash and tone onsets must match within 150 ms in all three HLS
renditions and the shared FLV output. Markers are paired in order, with missing
or extra markers rejected, so a whole-period shift cannot pass. This checks
media content alignment in addition to packet timestamps; the native packet
sink still does not test browser playback or external transport.

For a production encoding-group queue overflow, enable
`FFPLAYOUT_TIMING_DIAGNOSTICS=true` (or `--timing-diagnostics`). The overflow
warning then includes the worker's current stage, zero-based rendition index,
stage age, and last consumed audio/video PTS in microseconds. `video_submit` and
`audio_submit` include waiting for the internal encoder queues; `packet_handoff`
includes copy-recording and packet fan-out. These failure details work at the
normal warning level; periodic timing reports still require trace logging.
Worker tracking is disabled by default and does not emit per-frame logs.

Live timestamp recovery also checks the counterpart track after a suspected
clock reset. If buffered audio/video confirms the original source timeline,
the stale negative per-track offset is released without moving output backward.
The recovery logs `timestamp mapping recovered from counterpart`; an isolated
timestamp jump must not leave a permanent audio/video offset.
Correction warnings identify `stage=decode` or `stage=playout` and include
`expected_s`, `reference_s` (the normalized counterpart end timestamp), and
`offset_s` (the track's active source-to-normalized offset). These are separate
mapping stages; the previous identical messages could not distinguish them.

Forward gaps up to five seconds also retain their source timestamps when the
counterpart track has advanced into the gap by more than two frame durations
since the preceding frame, and a second frame confirms the jump. A small,
constant audio lead or normal per-frame progress does not confirm a gap.
The first candidate follows the existing outlier handling; an isolated bad
timestamp must not shift subsequent frames.
This avoids permanently compressing a missing video GOP while buffered audio
continues. Larger unsupported jumps and backward clock resets still use
discontinuity recovery.

The video scaler is created from the first usable decoded frame, rather than
potentially incomplete stream metadata. Invalid dimensions or missing pixels
produce a handled input error. If an accepted input announces video but only
delivers audio, audio and queue activity cannot extend the ten-second video
startup deadline. This deadline starts after opening/probing the input; it does
not limit waiting for a publisher to connect.

FFmpeg reports RTMP publisher stops as `EIO` as well as EOF. For live RTMP/RTMPS
listeners, an `EIO` packet-read result logs `input disconnected` at info level
and drains buffered packets before returning to playlist playback. This does
not distinguish a clean sender stop from a transport interruption. Other
read errors, other protocols and decoder failures retain their error handling.

### Media analysis and clip resource lifetimes

Metadata probes read stream parameters without opening additional decoders.
Playlist validation reuses probes from preceding entries in the same validation
run; external audio is matched by its own source path. A new validation run
starts without probes from the previous playlist. Fresh and reused probes apply
the same duration correction while preserving explicit clip boundaries.

Background silence checks run one at a time per process. Channel restarts and
shutdown cancel the validator as well as permanent stops. Cancellation of a
validator also interrupts its FFmpeg input/seek/read operations and is checked
between packets. The analysis slot is released by the blocking worker itself,
so aborting its async caller cannot start overlapping replacement analyses.
This may serialize validation across channels; it does not serialize playback.

Playback input workers are scoped to a clip generation and joined before the
next generation starts. Packet/frame queues and decoder/filter state then drop
with that generation. Output encoders and their bounded queues deliberately
remain alive across clips. A flat live heap with rising RSS still needs to be
distinguished from retained references; these lifecycle guarantees alone do
not prove a stable RSS over days.

### Linux allocator diagnostics

On Linux with glibc, the same `[Process Diagnostics]` records additionally report:

| Field | Meaning |
| --- | --- |
| `glibc_arena_bytes` | Arena space managed by glibc, including used and free blocks. |
| `glibc_used_bytes` | Arena bytes glibc accounts as in use (`uordblks`), excluding the separate mmap counter. |
| `glibc_free_bytes` | Free blocks retained inside those arenas (`fordblks`). |
| `glibc_mmap_bytes` | Allocator-managed mmap allocations (`hblkhd`), separate from arena space. |

These counters use `mallinfo2()`, available since glibc 2.33. The symbol is resolved
at runtime so older installations can still start; unsupported platforms,
missing symbols and fully static builds that do not export it report
`unavailable`. No extra package is required for these counters. Sampling uses
the existing blocking diagnostics worker, only with `--process-diagnostics`
and DEBUG logging. It runs at clip/live events and every five minutes; no new
timer, background thread, allocator tuning or forced memory release is added.

Compare the counters after warmup and at equivalent points in repeated workloads:
growing `glibc_free_bytes` with stable used/mmap counters supports allocator
retention; growing used/mmap counters warrants investigating retained allocations.
`glibc_used_bytes + glibc_mmap_bytes` is an allocator-accounted used total, **not**
an exact count of live application objects. Thread caches can still count as used;
allocator overhead is included. Free bytes are not necessarily immediately
returnable to the OS. Arena/mmap counters are not RSS or PSS and omit direct
mappings, stacks and allocations serviced by replacement allocators. When using
jemalloc/tcmalloc or another replacement allocator, do not interpret these as
whole-process heap measurements. The counters are observations, not a leak verdict.

See the glibc [`mallinfo2` implementation](https://github.com/bminor/glibc/blob/glibc-2.33/malloc/malloc.c)
for its arena accounting and the [allocation statistics manual](https://man7.org/linux/man-pages/man3/mallinfo.3.html)
for field definitions.

### Linux heap profiles

Use `scripts/heap-profile.sh` to launch a separate reproduction under
[Heaptrack](https://github.com/KDE/heaptrack). Install your distribution's
`heaptrack` package first; analysis can be done on a different machine with
`heaptrack_gui`. Use a release build with symbols (`scripts/build.sh --profiling …`
or `CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=false cargo build --release -p ffplayout`).
The executable must dynamically link its allocator for Heaptrack's preload
interception; statically linked FFmpeg libraries are fine. Keep the exact binary
and any separate debug symbols/shared libraries used during recording.

From the repository root, with a dedicated test database and output destination:

```sh
scripts/heap-profile.sh --output-dir /tmp/ffplayout-heap -- \
    ./target/release/ffplayout \
    --db /path/to/test.db --channel 1 --foreground
```

The script preserves the working directory and argument boundaries, enables
`FFPLAYOUT_PROCESS_DIAGNOSTICS=true`, and defaults to DEBUG console logging with
timestamps. Explicit CLI logging options are preserved; selecting a level above
DEBUG disables the process measurements. A unique private directory contains
the compressed `heaptrack.gz` or `heaptrack.zst` profile, combined `playout.log`,
command, binary SHA-256, system/tool version and exit code. It never overwrites
an earlier recording. The application's original umask is preserved, so HLS and
other application outputs retain their normal permissions. With no application
arguments, the script explicitly preserves ffplayout's default listener at
`127.0.0.1:8787` (or the `LISTEN` environment value).
It does not copy or anonymize the database/configuration;
the command you supply runs normally with those paths and outputs. Avoid running
a second instance against an active service's database, listeners or destinations.

Stop the foreground test normally with Ctrl+C when enough clip repetitions have
been collected. The log reader remains active while Heaptrack handles SIGINT,
and the script waits for recording finalization before saving the exit code.
There is no automatic duration, service restart or attachment to
existing processes. For a service, use a separately scheduled reproduction with
the same user/environment and workload. Runtime attachment is deliberately not
provided: upstream Heaptrack flags injection/detachment as unstable. This script
is a diagnostic tool, not a periodic restart mechanism for production.

Open the resulting file, using its actual `.gz` or `.zst` suffix:

```sh
heaptrack_gui /tmp/ffplayout-heap/ffplayout-TIMESTAMP-SUFFIX/heaptrack.zst
```

Compare retained allocations and their call stacks across repeated sequences
after warmup. Allocations still present when a running stream is interrupted
include legitimate decoder/encoder buffers; an end-of-profile “leak” count alone
is not proof of a leak. Profiling adds CPU/memory overhead and can produce large
files over hours, so check available disk space and use bounded reproductions.
The script does not change kernel security settings, install packages or run sudo.
On macOS, use Instruments Allocations; this helper explicitly supports Linux only.


## Graphics crate

The internal `ff-graphics` workspace crate owns native text rasterization, logo
preparation, and the CPU compositor. Engine integration remains in `ff-engine`.
See [graphics architecture](development/graphics-architecture.md) for module
boundaries, the optional FFmpeg backend, compatibility adapters and validation.
