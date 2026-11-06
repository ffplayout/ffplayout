# glibc and musl benchmark results

Build and measurement instructions are documented in the
[developer guide](../developer.md#compare-glibc-and-musl-performance).

The initial comparison on 2026-11-06 used an Intel Core i9-9900, with execution
pinned to four logical CPUs. All 15 cases ran in three fresh-process rounds per
libc, with seven samples of five iterations and four Rayon workers: 90 process
runs in total. Both builds used Rust 1.99.1 and FFmpeg revision
`b204e94e90fca64045e9ca7dd44c65678c7eb29e`. The Debian build used GCC 14.2;
the Alpine build used GCC 15.2. All sampled output checksums matched.

Selected median wall times and whole-process peak RSS:

| Operation | glibc time | musl time | glibc peak RSS | musl peak RSS |
| --- | ---: | ---: | ---: | ---: |
| 1080p preparation, serial | 12.385 ms | 18.802 ms | 66.3 MiB | 46.4 MiB |
| 1080p preparation, parallel | 5.319 ms | 14.147 ms | 67.1 MiB | 46.4 MiB |
| 4K preparation, parallel | 40.272 ms | 50.457 ms | 197.8 MiB | 177.1 MiB |
| 1080p compositing, serial | 0.480 ms | 0.503 ms | 15.4 MiB | 11.4 MiB |
| 4K compositing, parallel | 2.032 ms | 2.025 ms | 40.8 MiB | 36.7 MiB |
| Audio dynamics, one channel | 7.044 ms | 7.733 ms | 7.6 MiB | 3.1 MiB |
| Audio dynamics, four parallel channels | 7.776 ms | 8.259 ms | 9.2 MiB | 4.9 MiB |

Audio timings cover 1.024 seconds per channel. In this comparison, musl had
lower peak RSS in every case. Most processing took longer, especially 1080p
preparation; parallel 4K compositing was effectively tied. These results show
a memory/processing-time tradeoff for these builds, rather than a general
performance advantage for either libc. Preparation measurements include
allocation and cleanup, so their differences cannot be attributed to scaling
arithmetic alone.

Separately, a developer-reported one-hour playout test with short clips and
graphic transitions found substantially lower RAM usage with the musl build.
Its memory usage also reached a stable plateau earlier than the glibc build.
This sustained workload complements the short kernel benchmarks, whose peak
RSS measurements cannot describe memory retention over an hour. Exact RAM
values and a time series were not recorded here, so this remains a qualitative
observation rather than a quantified long-duration benchmark. It is consistent
with different allocator retention behavior, but does not by itself establish
the cause or rule out memory leaks.
