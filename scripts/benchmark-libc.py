#!/usr/bin/env python3
"""Build and compare production kernels against glibc and musl on one Linux host."""
import argparse
import csv
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def execute(command, **kwargs):
    return subprocess.run(command, check=True, text=True, **kwargs)


def build(args, output, variant, image):
    binary_dir = output / 'bin' / variant
    binary_dir.mkdir(parents=True, exist_ok=True)
    image_id = execute([args.container_cli, 'image', 'inspect', '--format', '{{.Id}}', image], capture_output=True).stdout.strip()
    (binary_dir / 'image.txt').write_text(f'{image}\n{image_id}\n')
    command = [args.container_cli, 'run', '--rm', '--security-opt', 'label=disable',
               '-v', f'{ROOT}:/src:ro', '-v', f'{output}:/results',
               '-e', f'CARGO_BUILD_JOBS={args.jobs}',
               '-e', f'LIBC_BENCH_CACHE_KEY={image_id.split(":")[-1][:16]}']
    if args.registry:
        command += ['-v', f'{Path(args.registry).resolve()}:/usr/local/cargo/registry:ro', '-e', 'CARGO_NET_OFFLINE=true']
    command += ['--entrypoint', 'sh', image, '/src/scripts/benchmark-libc-build.sh', variant]
    print(f'Building {variant} with {image}', flush=True)
    with (output / f'{variant}-build.log').open('w') as log:
        execute(command, stdout=log, stderr=subprocess.STDOUT)
    for suite in ['graphics', 'audio']:
        artifacts = [json.loads(line) for line in (binary_dir / f'{suite}-build.jsonl').read_text().splitlines()]
        candidates = [item['executable'] for item in artifacts if item.get('reason') == 'compiler-artifact'
                      and item.get('target', {}).get('name') == f'libc_{suite}' and item.get('executable')]
        if len(candidates) != 1:
            raise RuntimeError(f'Expected one {suite} executable, found {candidates}')
        relative = Path(candidates[0]).relative_to('/results')
        shutil.copy2(output / relative, binary_dir / suite)
    # Inspect the actual ELF files, not just compiler settings.
    for suite in ['graphics', 'audio']:
        binary = binary_dir / suite
        headers = execute(['readelf', '-lW', str(binary)], capture_output=True).stdout
        dynamic = execute(['readelf', '-dW', str(binary)], capture_output=True).stdout
        (binary_dir / f'{suite}-elf.txt').write_text(headers + dynamic)
        has_dynamic = 'INTERP' in headers or '(NEEDED)' in dynamic
        if has_dynamic != (variant == 'glibc'):
            raise RuntimeError(f'{variant}/{suite} has unexpected ELF linkage')


def benchmark(args, output):
    catalog = {}
    for suite in ['graphics', 'audio']:
        for variant in ['glibc', 'musl']:
            binary = output / 'bin' / variant / suite
            cases = execute([str(binary), '--list'], capture_output=True).stdout.splitlines()
            if suite in catalog and catalog[suite] != cases:
                raise RuntimeError(f'Case lists differ between libc builds: {suite}')
            catalog[suite] = cases
    (output / 'run-environment.json').write_text(json.dumps({
        'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'arguments': {key: str(value) for key, value in vars(args).items()},
        'cpu_affinity': sorted(os.sched_getaffinity(0)),
    }, indent=2) + '\n')
    measurements = []
    environment = dict(os.environ)
    # Compare standard allocators without inherited glibc-specific tuning/preloads.
    for name in list(environment):
        if name.startswith('MALLOC_') or name in ['GLIBC_TUNABLES', 'LD_PRELOAD', 'LD_LIBRARY_PATH']:
            del environment[name]
    environment['RAYON_NUM_THREADS'] = str(args.threads)
    with (output / 'measurements.jsonl').open('w') as raw:
        for round_index in range(args.rounds):
            for suite, cases in catalog.items():
                for index, case in enumerate(cases):
                    if args.filter and args.filter not in case:
                        continue
                    variants = ['glibc', 'musl'] if (round_index + index) % 2 == 0 else ['musl', 'glibc']
                    for variant in variants:
                        command = [str(output / 'bin' / variant / suite), '--case', case,
                                   '--samples', str(args.samples), '--iterations', str(args.iterations),
                                   '--warmup', str(args.warmup), '--threads', str(args.threads)]
                        print(f'Round {round_index + 1}/{args.rounds}: {variant} {case}', flush=True)
                        result = execute(command, env=environment, capture_output=True)
                        row = json.loads(result.stdout)
                        if row['libc'] != variant:
                            raise RuntimeError(f'Wrong libc executable: {variant}/{suite}')
                        row.update(suite=suite, round=round_index + 1)
                        measurements.append(row)
                        raw.write(json.dumps(row) + '\n')
                        raw.flush()
    if not measurements:
        raise RuntimeError('No benchmark cases selected')
    summarize(measurements, output)


def summarize(rows, output):
    summaries = []
    for suite, case in sorted({(row['suite'], row['case']) for row in rows}):
        variants = {variant: [row for row in rows if row['suite'] == suite and row['case'] == case
                              and row['libc'] == variant] for variant in ['glibc', 'musl']}
        versions = {row['avutil_version'] for values in variants.values() for row in values}
        if len(versions) != 1:
            raise RuntimeError(f'FFmpeg versions differ for {case}; rebuild matching builder images')
        summary = {'suite': suite, 'case': case}
        for variant, values in variants.items():
            for metric in ['wall_median_ms', 'wall_p95_ms', 'cpu_median_ms', 'peak_rss_mib']:
                summary[f'{variant}_{metric}'] = statistics.median(row[metric] for row in values)
            times = [row['wall_median_ms'] for row in values]
            summary[f'{variant}_wall_min_ms'] = min(times)
            summary[f'{variant}_wall_max_ms'] = max(times)
        summary['musl_wall_change_percent'] = (summary['musl_wall_median_ms'] / summary['glibc_wall_median_ms'] - 1) * 100
        summary['checksum_equal'] = {row['checksum'] for row in variants['glibc']} == {row['checksum'] for row in variants['musl']}
        summaries.append(summary)
    with (output / 'summary.csv').open('w', newline='') as file:
        writer = csv.DictWriter(file, fieldnames=list(summaries[0]))
        writer.writeheader()
        writer.writerows(summaries)
    context = []
    metadata_path = output / 'environment.json'
    if metadata_path.exists():
        metadata = json.loads(metadata_path.read_text())
        model = next((line.split(':', 1)[1].strip() for line in metadata['cpu'].splitlines()
                      if line.startswith('model name')), metadata['platform'])
        context = [f"CPU: {model}. Revision: `{metadata['revision']}`.", '']
    lines = ['# glibc / musl kernel benchmarks', '', *context,
             f"Rounds: {len(variants['glibc'])}; samples per round: {rows[0]['samples']}; iterations per sample: {rows[0]['iterations']}; pool size: {rows[0]['threads']}.",
             'Graphics times cover one operation. Audio times cover 1.024 seconds per channel (multiple-channel cases use the pool size as channel count).', '',
             'Positive differences mean musl took longer. Values are medians across fresh-process rounds.', '',
             '| Case | glibc ms | musl ms | musl change | CPU glibc/musl ms | Peak RSS glibc/musl MiB | Checksum equal |',
             '|---|---:|---:|---:|---:|---:|---|']
    for row in summaries:
        lines.append(f"| {row['case']} | {row['glibc_wall_median_ms']:.3f} | {row['musl_wall_median_ms']:.3f} | "
                     f"{row['musl_wall_change_percent']:+.1f}% | {row['glibc_cpu_median_ms']:.3f}/{row['musl_cpu_median_ms']:.3f} | "
                     f"{row['glibc_peak_rss_mib']:.1f}/{row['musl_peak_rss_mib']:.1f} | {row['checksum_equal']} |")
    lines += ['', 'See summary.csv for p95 and the spread between rounds; measurements.jsonl contains every round.',
              'RSS is the whole-process high-water mark, including setup; it does not measure a leak or post-cleanup retention.',
              'These compare the recorded complete builds, including native libraries and toolchains, not an isolated libc substitution.']
    (output / 'report.md').write_text('\n'.join(lines) + '\n')
    print('\n'.join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--container-cli', default=os.environ.get('CONTAINER_CLI') or shutil.which('podman') or 'docker')
    parser.add_argument('--glibc-image', default='localhost/ffplayout-static-builder:latest')
    parser.add_argument('--musl-image', default='localhost/ffplayout-musl-builder:latest')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/libc-benchmarks')
    parser.add_argument('--registry', help='Optional local Cargo registry for offline container builds')
    parser.add_argument('--jobs', type=int, default=2)
    parser.add_argument('--rounds', type=int, default=5)
    parser.add_argument('--samples', type=int, default=15)
    parser.add_argument('--iterations', type=int, default=10)
    parser.add_argument('--warmup', type=int, default=3)
    parser.add_argument('--threads', type=int, default=4)
    parser.add_argument('--filter', default='')
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--run-only', action='store_true')
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() not in ['x86_64', 'AMD64']:
        parser.error('Run on native x86-64 Linux, without CPU emulation')
    if args.samples < 3 or min(args.rounds, args.iterations, args.jobs, args.threads) < 1 or args.warmup < 0:
        parser.error('Invalid measurement counts')
    if args.build_only and args.run_only:
        parser.error('Choose build-only or run-only')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if not args.run_only:
        metadata = {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    'platform': platform.platform(), 'cpu': Path('/proc/cpuinfo').read_text(),
                    'revision': execute(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], capture_output=True).stdout.strip(),
                    'working_diff_sha256': hashlib.sha256(execute(['git', '-C', str(ROOT), 'diff', 'HEAD'], capture_output=True).stdout.encode()).hexdigest(),
                    'benchmark_source_sha256': {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                                                for path in [ROOT / 'benches/support/mod.rs', ROOT / 'crates/graphics/benches/libc_graphics.rs', ROOT / 'crates/engine/benches/libc_audio.rs']},
                    'arguments': {key: str(value) for key, value in vars(args).items()}}
        (output / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
        for variant in ['glibc', 'musl']:
            build(args, output, variant, getattr(args, f'{variant}_image'))
    if not args.build_only:
        benchmark(args, output)


if __name__ == '__main__':
    try:
        main()
    except (subprocess.CalledProcessError, RuntimeError) as error:
        print(f'Benchmark failed: {error}', file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        sys.exit(1)
