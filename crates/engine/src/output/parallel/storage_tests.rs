//! Opt-in destructive-capacity tests: use only a dedicated disposable filesystem.
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ffmpeg_next::{
    ChannelLayout,
    format::{Pixel, Sample, sample::Type},
    frame,
};

use super::{OutputConfig, OutputDestination, OutputTarget, ParallelOutput};

fn shared_output(
    directory: &std::path::Path,
    healthy: &std::path::Path,
) -> (ParallelOutput, super::OutputStatus) {
    let mut config = OutputConfig::new(160, 90, 25, 48_000);

    config.stream_type = crate::StreamType::Custom;
    config.stream_format = "mpegts".into();
    config
        .video_options
        .insert("preset".into(), "ultrafast".into());
    config
        .video_options
        .insert("tune".into(), "zerolatency".into());

    let mut native = config.clone();

    native
        .muxer_options
        .insert("ffplayout_packager".into(), "native".into());
    config.destinations.push(OutputDestination {
        id: 1,
        config: native,
        target: OutputTarget::Hls {
            playlist: directory.join("stream.m3u8").to_string_lossy().into_owned(),
            variants: vec![],
            subtitle: None,
            segment_seconds: 1,
            list_size: 6,
        },
    });

    let mut borrowed = config.clone();

    borrowed.destinations.clear();
    borrowed.encoding_source = Some((1, 0));
    config.destinations.push(OutputDestination {
        id: 2,
        config: borrowed,
        target: OutputTarget::Stream {
            url: healthy.to_string_lossy().into_owned(),
        },
    });

    let status = config.output_status.clone();
    let output = ParallelOutput::new(&config).unwrap();

    assert_eq!(
        output.workers.lock().unwrap().len(),
        1,
        "must share the encoder"
    );

    (output, status)
}

fn send_frame(output: &ParallelOutput, index: i64) {
    let mut video = frame::Video::new(Pixel::YUV420P, 160, 90);

    for plane in 0..video.planes() {
        video.data_mut(plane).fill(100 + (index % 30) as u8);
    }

    video.set_pts(Some(index));
    output
        .encode_video(&video)
        .unwrap_or_else(|error| panic!("{error:#}: {:?}", output.config.output_status.snapshot()));

    let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);

    audio.set_rate(48_000);
    audio.set_pts(Some(index * 1920));

    for channel in 0..2 {
        audio.plane_mut::<f32>(channel).fill(0.1);
    }

    output.encode_audio(&audio).unwrap();
}

#[test]
#[ignore = "requires a dedicated small RAM disk: FFPLAYOUT_STORAGE_TEST_ROOT"]
fn full_filesystem_preserves_history_and_shared_encoder_output_then_recovers() {
    ffmpeg_next::init().unwrap();

    let root = PathBuf::from(
        std::env::var_os("FFPLAYOUT_STORAGE_TEST_ROOT").expect("dedicated RAM disk required"),
    );

    assert!(
        root.join(".ffplayout-storage-test").is_file(),
        "missing disposable-volume marker"
    );

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = root.join(format!("hls-{unique}"));
    let healthy = std::env::temp_dir().join(format!("healthy-storage-{unique}.ts"));

    fs::create_dir(&directory).unwrap();

    let (output, status) = shared_output(&directory, &healthy);

    let started = Instant::now();
    let filler_path = directory.join("fill.disk");
    let mut before = Vec::new();
    let mut saw_failure = false;
    let mut recovered = false;

    for index in 0..650_i64 {
        send_frame(&output, index);

        if index == 100 {
            assert!(
                directory.join("stream.m3u8").is_file(),
                "missing HLS history: {:?}",
                status.snapshot()
            );

            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();

                if path.extension().is_some_and(|ext| ext == "m3u8") {
                    let playlist = fs::read_to_string(&path).unwrap();

                    for uri in playlist
                        .lines()
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    {
                        let segment = directory.join(uri);

                        before.push((segment.clone(), fs::read(segment).unwrap()));
                    }

                    before.push((path, playlist.into_bytes()));
                }
            }

            let mut filler = File::create(&filler_path).unwrap();
            let block = [1_u8; 65536];
            let mut written = 0;

            loop {
                match filler.write_all(&block) {
                    Ok(()) => written += block.len(),
                    Err(error) => {
                        assert_eq!(error.kind(), std::io::ErrorKind::StorageFull, "{error}");
                        eprintln!("Real filesystem full after {written} filler bytes: {error}");
                        break;
                    }
                }

                assert!(
                    written < 96 * 1024 * 1024,
                    "refusing to fill a large filesystem"
                );
            }
        }

        if (125..200).contains(&index) {
            let states = status.snapshot();

            if let Some(error) = states.get(&1).and_then(|state| state.error.as_ref()) {
                if !saw_failure {
                    eprintln!("Native HLS storage failure: {error}");
                }

                assert_eq!(states[&2].phase, "running");
                saw_failure = true;
            }
        }

        if index == 200 {
            assert!(saw_failure, "HLS did not report the full filesystem");

            for (path, contents) in &before {
                assert_eq!(
                    &fs::read(path).unwrap(),
                    contents,
                    "history changed while full"
                );
            }

            fs::remove_file(&filler_path).unwrap();
        }

        if index > 200 {
            recovered |= status
                .snapshot()
                .get(&1)
                .is_some_and(|state| state.phase == "running" && state.error.is_none());
        }

        std::thread::sleep(
            (started + Duration::from_millis((index as u64 + 1) * 40))
                .saturating_duration_since(Instant::now()),
        );
    }

    assert!(
        recovered,
        "native output did not recover after space became available: {:?}",
        status.snapshot()
    );
    output.finish().unwrap();

    let playlist = fs::read_to_string(directory.join("stream.m3u8")).unwrap();

    for uri in playlist
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut segment = ffmpeg_next::format::input(&directory.join(uri)).unwrap();
        let video_packets = segment
            .packets()
            .filter(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
            .count();

        assert!(video_packets > 0, "recovered segment has no video: {uri}");
    }

    let mut input = ffmpeg_next::format::input(&healthy).unwrap();
    let video_packets = input
        .packets()
        .filter(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
        .count();

    assert_eq!(video_packets, 650, "healthy shared output lost video");
    drop(input);
    fs::remove_file(healthy).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires a controlled dm-delay filesystem and FFPLAYOUT_STORAGE_CONTROL"]
fn slow_filesystem_keeps_shared_output_running_and_recovers() {
    ffmpeg_next::init().unwrap();

    let root = PathBuf::from(std::env::var_os("FFPLAYOUT_STORAGE_TEST_ROOT").unwrap());
    let control = PathBuf::from(std::env::var_os("FFPLAYOUT_STORAGE_CONTROL").unwrap());

    assert!(root.join(".ffplayout-storage-test").is_file());
    assert!(control.is_dir());

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = root.join(format!("slow-{unique}"));
    let healthy = std::env::temp_dir().join(format!("healthy-slow-{unique}.ts"));

    fs::create_dir(&directory).unwrap();

    let (output, status) = shared_output(&directory, &healthy);
    let started = Instant::now();
    let mut failures = Vec::new();

    fs::write(control.join("started"), b"ready").unwrap();

    for index in 0..1500_i64 {
        send_frame(&output, index);

        if index >= 25 && index % 25 == 0 {
            let states = status.snapshot();

            assert_eq!(states[&2].phase, "running", "healthy output: {states:?}");

            if let Some(error) = states[&1].error.as_ref()
                && failures.last() != Some(error)
            {
                failures.push(error.clone());
                eprintln!("slow storage at frame {index}: {error}");
            }
        }

        std::thread::sleep(
            (started + Duration::from_millis((index as u64 + 1) * 40))
                .saturating_duration_since(Instant::now()),
        );
    }

    assert!(
        control.join("slow-active").is_file(),
        "controller never applied delay"
    );
    assert!(
        control.join("restored").is_file(),
        "controller never removed delay"
    );
    assert_eq!(
        status.snapshot()[&1].phase,
        "running",
        "HLS did not recover: {:?}",
        status.snapshot()
    );
    assert!(status.snapshot()[&1].error.is_none());
    output.finish().unwrap();

    let mut input = ffmpeg_next::format::input(&healthy).unwrap();
    let video_packets = input
        .packets()
        .filter(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
        .count();

    assert_eq!(
        video_packets, 1500,
        "healthy output lost video under slow I/O"
    );
    drop(input);

    let playlist = fs::read_to_string(directory.join("stream.m3u8")).unwrap();
    let mut latest_video_time = 0.0_f64;

    for uri in playlist
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut segment = ffmpeg_next::format::input(&directory.join(uri)).unwrap();

        let mut videos = 0;

        for (stream, packet) in segment.packets() {
            if stream.parameters().medium() == ffmpeg_next::media::Type::Video {
                videos += 1;

                if let Some(pts) = packet.pts() {
                    latest_video_time =
                        latest_video_time.max(pts as f64 * f64::from(stream.time_base()));
                }
            }
        }

        assert!(videos > 0, "recovered segment has no video: {uri}");
    }

    assert!(
        latest_video_time >= 59.0,
        "HLS stayed behind the program: {latest_video_time}"
    );
    eprintln!(
        "slow filesystem: {video_packets} healthy video packets, HLS reached {latest_video_time:.3} s, {} distinct HLS errors, final state {:?}",
        failures.len(),
        status.snapshot()
    );
    fs::remove_file(healthy).unwrap();
    fs::remove_dir_all(directory).unwrap();
}
