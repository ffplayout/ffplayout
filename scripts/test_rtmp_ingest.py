#!/usr/bin/env python3
"""Exercise a running RTMP ingest with normal pacing, a pause, or a catch-up burst.

Requires Python 3 and FFmpeg with libx264/AAC. Encodes the source before the
test, then feeds timestamped FLV packets to a separate FFmpeg RTMP publisher.
No reconnects are attempted: an unexpected disconnect must remain visible.
"""

import argparse
import json
import math
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit


def positive(value):
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return value


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", help="RTMP ingest URL, including application and stream key")
    parser.add_argument("--scenario", choices=("normal", "pause", "burst"), default="normal")
    parser.add_argument("--duration", type=positive, default=60, help="source seconds to send (default: 60)")
    parser.add_argument("--at", type=positive, default=10, help="source second at which pause/burst starts")
    parser.add_argument("--pause-seconds", type=positive, default=0.75)
    parser.add_argument("--burst-seconds", type=positive, default=15, help="source seconds sent at increased rate")
    parser.add_argument("--burst-rate", type=positive, default=1.5)
    parser.add_argument("--input", type=Path, help="optional video source; otherwise test pattern and tone")
    parser.add_argument("--source-flv", type=Path, help="reuse a clean H.264/AAC FLV instead of encoding")
    parser.add_argument("--size", default="1920x1080")
    parser.add_argument("--bitrate", default="5M")
    parser.add_argument("--log-dir", type=Path, help="new directory for source, publisher logs and events")
    parser.add_argument("--timeout", type=positive, help="maximum publishing wall time, including blocked writes")
    args = parser.parse_args()
    parsed = urlsplit(args.url)

    if parsed.scheme != "rtmp" or not parsed.hostname or "/" not in parsed.path.strip("/"):
        parser.error("URL must be rtmp://host:port/application/stream-key")
    if args.input and args.source_flv:
        parser.error("--input and --source-flv are mutually exclusive")
    if args.scenario != "normal" and args.at >= args.duration:
        parser.error("--at must be smaller than --duration")
    if args.scenario == "burst" and args.burst_rate <= 1:
        parser.error("--burst-rate must exceed 1")
    if not shutil.which("ffmpeg"):
        parser.error("ffmpeg is not installed")

    return args


def prepare_source(args, directory):
    if args.source_flv:
        return args.source_flv.resolve()

    source = directory / "source.flv"
    command = ["ffmpeg", "-hide_banner", "-nostdin", "-y"]

    if args.input:
        command += ["-stream_loop", "-1", "-i", str(args.input)]
        command += ["-vf", f"scale={args.size.replace('x', ':')},fps=25", "-map", "0:v:0", "-map", "0:a:0"]
    else:
        command += ["-f", "lavfi", "-i", f"testsrc2=size={args.size}:rate=25",
                    "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"]

    command += ["-t", str(args.duration), "-c:v", "libx264", "-preset", "ultrafast",
                "-tune", "zerolatency", "-pix_fmt", "yuv420p", "-threads", "2",
                "-g", "50", "-bf", "0", "-b:v", args.bitrate, "-maxrate", args.bitrate,
                "-bufsize", args.bitrate, "-c:a", "aac", "-b:a", "160k", "-ar", "48000",
                "-ac", "2", "-f", "flv", str(source)]
    print("Preparing source before publishing …", flush=True)

    with (directory / "prepare.log").open("w") as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)

    return source


def flv_tags(source):
    """Yield complete tags without rewriting their media timestamps."""
    with source.open("rb") as stream:
        header = stream.read(9)
        if header[:3] != b"FLV" or len(header) != 9:
            raise ValueError("source is not an FLV file")

        offset = int.from_bytes(header[5:9], "big")
        if not 9 <= offset <= 1024:
            raise ValueError("invalid FLV header size")

        initial = header + stream.read(offset - 9 + 4)
        if len(initial) != offset + 4:
            raise ValueError("truncated FLV header")

        yield None, 0, initial
        while header := stream.read(11):
            if len(header) != 11:
                raise ValueError("truncated FLV tag")

            length = int.from_bytes(header[1:4], "big")
            payload = stream.read(length + 4)
            if len(payload) != length + 4 or int.from_bytes(payload[-4:], "big") != length + 11:
                raise ValueError("invalid FLV tag size")

            stamp = int.from_bytes(header[4:7], "big") | header[7] << 24
            yield header[0], stamp / 1000, header + payload


def delivery_time(args, media_seconds):
    if args.scenario == "pause" and media_seconds >= args.at:
        return media_seconds + args.pause_seconds
    if args.scenario == "burst" and media_seconds > args.at:
        accelerated = min(media_seconds - args.at, args.burst_seconds)
        return media_seconds - accelerated * (1 - 1 / args.burst_rate)

    return media_seconds


def publish(args, source, directory):
    started = time.monotonic()
    event_lock = threading.Lock()
    events = (directory / "events.jsonl").open("w")
    stop = threading.Event()
    failures = []
    counters = {"bytes": 0, "video_packets": 0, "audio_packets": 0, "media_seconds": 0}

    def record(event, **details):
        with event_lock:
            events.write(json.dumps({"elapsed": round(time.monotonic() - started, 3),
                                     "event": event, **details}) + "\n")
            events.flush()

    command = ["ffmpeg", "-hide_banner", "-nostats", "-nostdin", "-loglevel", "info",
               "-analyzeduration", "0", "-probesize", "32768", "-fpsprobesize", "0",
               "-f", "flv", "-i", "pipe:0", "-map", "0:v:0", "-map", "0:a:0?",
               "-c", "copy", "-flush_packets", "1", "-flvflags", "no_duration_filesize",
               "-progress", "pipe:1", "-f", "flv", args.url]

    def feed(process):
        changed = resumed = False
        previous = 0
        last_status = started
        try:
            for kind, media_seconds, packet in flv_tags(source):
                if media_seconds >= args.duration:
                    break
                if kind in (8, 9) and media_seconds + 0.25 < previous:
                    raise ValueError("use a clean source: large backward timestamps detected")
                if kind in (8, 9):
                    previous = max(previous, media_seconds)

                target = delivery_time(args, previous)
                if args.scenario != "normal" and not changed and previous >= args.at:
                    changed = True
                    record(args.scenario + "_start", media_seconds=previous,
                           pause_seconds=args.pause_seconds if args.scenario == "pause" else None,
                           rate=args.burst_rate if args.scenario == "burst" else None)
                    print(f"{args.scenario} starts at source {previous:.3f}s", flush=True)

                if stop.wait(max(0, started + target - time.monotonic())):
                    return

                if changed and not resumed and (args.scenario == "pause" or previous >= args.at + args.burst_seconds):
                    resumed = True
                    record("normal_pacing_resumed", media_seconds=previous)
                    print("Normal pacing resumes", flush=True)

                before = time.monotonic()
                process.stdin.write(packet)
                process.stdin.flush()
                blocked = time.monotonic() - before
                counters["bytes"] += len(packet)
                counters["video_packets"] += kind == 9
                counters["audio_packets"] += kind == 8
                counters["media_seconds"] = previous
                if blocked >= 0.25:
                    record("publisher_input_blocked", seconds=round(blocked, 3), media_seconds=previous)

                if time.monotonic() - last_status >= 1:
                    last_status = time.monotonic()
                    record("source_progress", **counters)

            if counters["media_seconds"] < args.duration - 0.25:
                raise ValueError("source is shorter than --duration; regenerate it or reduce the duration")

            record("source_complete", **counters)
        except (OSError, ValueError) as error:
            failures.append(str(error))
            record("source_error", error=str(error), **counters)
        finally:
            try:
                process.stdin.close()
            except OSError:
                pass

    timeout = args.timeout or args.duration + args.pause_seconds + 30
    record("start", scenario=args.scenario, duration=args.duration, at=args.at,
           pause_seconds=args.pause_seconds, burst_seconds=args.burst_seconds,
           burst_rate=args.burst_rate, timeout=timeout)
    try:
        with (directory / "publisher.log").open("w") as stderr, (directory / "progress.log").open("w") as stdout:
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=stderr)
            # Progress has its own reader so a full stdout pipe cannot block RTMP.
            def read_progress():
                for line in process.stdout:
                    stdout.write(f"{time.monotonic() - started:.3f} {line.decode(errors='replace')}")
                    stdout.flush()

            feeder = threading.Thread(target=feed, args=(process,), daemon=True)
            reader = threading.Thread(target=read_progress, daemon=True)
            feeder.start()
            reader.start()
            try:
                code = process.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                record("timeout_or_interruption", **counters)
                stop.set()
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                code = 124
            finally:
                stop.set()
                feeder.join(timeout=3)
                reader.join(timeout=3)

        record("exit", returncode=code, errors=failures, **counters)
    finally:
        events.close()

    print(f"Publisher exit: {code}; delivered source: {counters['media_seconds']:.3f}s", flush=True)
    print(f"Logs: {directory}", flush=True)
    if failures:
        print("; ".join(failures), flush=True)
    print("Also check ffplayout logs: successful publishing alone does not prove uninterrupted playback.", flush=True)

    return code if code else int(bool(failures))


def main():
    args = arguments()
    directory = args.log_dir.resolve() if args.log_dir else None
    try:
        if directory:
            directory.mkdir(parents=True, exist_ok=False)
        else:
            directory = Path(tempfile.mkdtemp(prefix="ffplayout-rtmp-test-"))

        source = prepare_source(args, directory)
        return publish(args, source, directory)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Test failed: {error}\nLogs: {directory}", flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
