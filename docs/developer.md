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
