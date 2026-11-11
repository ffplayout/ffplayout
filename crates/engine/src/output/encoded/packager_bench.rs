//! Opt-in comparison using identical pre-encoded packets for both HLS backends.
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{Packet, Rational, Rescale, codec, codec::packet::Mut, format, rescale};

use crate::{HlsSubtitle, HlsVariant, OutputConfig, utils::ffmpeg::reference_packet};

use super::{EncodedFormat, EncodedOutput};

struct Sample {
    parameters: Vec<(codec::Parameters, codec::Parameters)>,
    packets: Vec<(usize, Rational, Packet)>,
}

impl Sample {
    fn load(path: &Path, variants: usize) -> Result<Self> {
        let mut input = format::input(path)?;
        let parameters = (0..variants)
            .map(|index| {
                Ok((
                    input
                        .stream(index * 2)
                        .context("video stream missing")?
                        .parameters()
                        .clone(),
                    input
                        .stream(index * 2 + 1)
                        .context("audio stream missing")?
                        .parameters()
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?;
        let packets = input
            .packets()
            .filter(|(stream, packet)| {
                stream.index() < variants * 2 && packet.pts().is_some_and(|pts| pts >= 0)
            })
            .map(|(stream, packet)| (stream.index(), stream.time_base(), packet))
            .collect();

        Ok(Self {
            parameters,
            packets,
        })
    }
}

fn usage() -> libc::rusage {
    let mut usage = std::mem::MaybeUninit::zeroed();
    // SAFETY: getrusage initializes the correctly sized, writable destination.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };

    assert_eq!(result, 0);

    // SAFETY: a successful getrusage initialized all fields.
    unsafe { usage.assume_init() }
}

fn cpu_seconds(usage: &libc::rusage) -> f64 {
    usage.ru_utime.tv_sec as f64
        + usage.ru_stime.tv_sec as f64
        + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.0
}

fn option(name: &str, default: u32) -> u32 {
    env::var(name).map_or(default, |value| {
        value.parse().expect("invalid benchmark option")
    })
}

/// See docs/development/hls-packager-comparison.md for fixture and invocation.
#[test]
#[ignore = "requires a six-track encoded fixture and writes HLS media"]
fn compare_hls_packagers() -> Result<()> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);

    let source = PathBuf::from(env::var("HLS_BENCH_SOURCE")?);
    let directory = PathBuf::from(env::var("HLS_BENCH_DIRECTORY")?);
    let native = env::var("HLS_BENCH_BACKEND").as_deref() == Ok("native");
    let variants = option("HLS_BENCH_VARIANTS", 3) as usize;
    let seconds = option("HLS_BENCH_SECONDS", 120);
    let segment_seconds = option("HLS_BENCH_SEGMENT_SECONDS", 6);
    let list_size = option("HLS_BENCH_LIST_SIZE", 600);
    let subtitles = option("HLS_BENCH_SUBTITLES", 1) != 0;
    let start_ms = i64::from(option("HLS_BENCH_START_MS", 0));
    let realtime = option("HLS_BENCH_REALTIME", 0) != 0;
    let skipped_keyframes: BTreeSet<i64> = env::var("HLS_BENCH_SKIP_KEYFRAME_MS")
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;

    ensure!(
        matches!(variants, 1 | 3),
        "benchmark expects one or three renditions"
    );
    ensure!(seconds.is_multiple_of(10), "fixture is ten seconds long");
    ensure!(!directory.exists(), "use a fresh benchmark directory");
    fs::create_dir_all(&directory)?;

    let sample = Sample::load(&source, variants)?;
    let mut config = OutputConfig::new(1920, 1080, 25, 48_000);

    if native {
        config
            .muxer_options
            .insert("ffplayout_packager".into(), "native".into());
    }

    let renditions: Vec<_> = [
        ("high", 1920, 1080, 4_500_000),
        ("mid", 1280, 720, 2_400_000),
        ("low", 854, 480, 1_200_000),
    ]
    .into_iter()
    .take(variants)
    .map(|(name, width, height, bitrate)| HlsVariant {
        name: name.into(),
        width,
        height,
        video_bitrate: bitrate,
        audio_bitrate: 128_000,
    })
    .collect();
    let output_format = EncodedFormat::Hls {
        variants: renditions,
        subtitle: subtitles.then(|| HlsSubtitle {
            name: "Captions".into(),
            language: "en".into(),
            default: true,
        }),
        segment_seconds,
        list_size,
    };
    let playlist = directory.join("master.m3u8");
    let path = playlist.to_str().context("invalid benchmark directory")?;
    let captions_media = directory.join("source.mp4");
    let mut captions = "WEBVTT\n\n".to_owned();

    for second in (0..seconds).step_by(10) {
        let stamp = |value: u32| {
            format!(
                "{:02}:{:02}:{:02}.000",
                value / 3600,
                value / 60 % 60,
                value % 60
            )
        };

        captions.push_str(&format!(
            "{} --> {}\nBenchmark caption {second}\n\n",
            stamp(second),
            stamp(second + 3)
        ));
    }

    fs::write(captions_media.with_extension("vtt"), captions)?;

    let baseline = usage();
    let started = Instant::now();
    let mut output = EncodedOutput::open_internal(
        path,
        &config,
        output_format.clone(),
        None,
        false,
        Some(sample.parameters.clone()),
    )?;

    if subtitles {
        output.write_vtt_subtitles(
            captions_media.to_str().unwrap(),
            0,
            0,
            Some(i64::from(seconds) * 1000),
        )?;
    }

    let mut first_visible = None;
    let mut first_dts = None;
    let mut packet_count = 0_u64;

    for repetition in 0..seconds / 10 {
        for (stream, time_base, packet) in &sample.packets {
            let mut packet = reference_packet(packet)?;
            // Each simulated encoder packet gets a fresh payload allocation.
            // Reusing fixture buffers would hide the native segment buffer's RSS.
            // SAFETY: this exclusively owned packet references a valid AVPacket.
            let writable =
                unsafe { ffmpeg_next::ffi::av_packet_make_writable(packet.as_mut_ptr()) };

            if writable < 0 {
                return Err(ffmpeg_next::Error::from(writable).into());
            }

            let offset = i64::from(repetition * 10).rescale(Rational(1, 1), *time_base);

            packet.set_pts(packet.pts().map(|pts| pts + offset));
            packet.set_dts(packet.dts().map(|dts| dts + offset));

            let position = packet
                .pts()
                .unwrap()
                .rescale(*time_base, rescale::TIME_BASE);

            if position / 1000 < start_ms {
                continue;
            }

            if packet.is_key() && skipped_keyframes.contains(&(position / 1000)) {
                packet.set_flags(packet.flags() & !ffmpeg_next::codec::packet::Flags::KEY);
            }

            if realtime {
                let dts = packet
                    .dts()
                    .unwrap_or(packet.pts().unwrap())
                    .rescale(*time_base, rescale::TIME_BASE);
                let origin = *first_dts.get_or_insert(dts);
                let due = Duration::from_micros(dts.saturating_sub(origin).max(0) as u64);

                thread::sleep(due.saturating_sub(started.elapsed()));
            }

            if subtitles {
                output.advance_vtt_subtitles(position / 1000)?;
            }

            output.write_packet(&mut packet, *stream, *time_base)?;
            packet_count += 1;

            if first_visible.is_none() && directory.join("high.m3u8").exists() {
                first_visible = Some(position as f64 / 1_000_000.0);
            }
        }
    }

    output.finish()?;

    let wall = started.elapsed().as_secs_f64();
    let finished = usage();
    let resume_started = Instant::now();
    let output = EncodedOutput::open_internal(
        path,
        &config,
        output_format,
        None,
        false,
        Some(sample.parameters.clone()),
    )?;
    let resume_ms = resume_started.elapsed().as_secs_f64() * 1000.0;

    drop(output);

    let rss_bytes = finished.ru_maxrss as u64 * if cfg!(target_os = "macos") { 1 } else { 1024 };
    let backend = if native { "native" } else { "ffmpeg" };
    let cpu = cpu_seconds(&finished) - cpu_seconds(&baseline);
    let first_visible = first_visible.map_or("null".into(), |value| format!("{value:.3}"));

    println!(
        "HLS_BENCH {{\"backend\":\"{backend}\",\"variants\":{variants},\"seconds\":{seconds},\"subtitles\":{subtitles},\"packets\":{packet_count},\"wall_s\":{wall:.6},\"cpu_s\":{cpu:.6},\"peak_rss_bytes\":{rss_bytes},\"resume_ms\":{resume_ms:.3},\"first_visible_media_s\":{first_visible}}}"
    );

    Ok(())
}
