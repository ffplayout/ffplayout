use std::{
    thread::{self, sleep},
    time::Duration,
};

use serial_test::serial;
use simplelog::*;

use ffplayout::{input::playlist::gen_source, output::player};
use ffplayout_lib::{utils::*, vec_strings};

fn timed_stop(sec: u64, proc_ctl: ProcessControl) {
    sleep(Duration::from_secs(sec));

    info!("Timed stop of process");

    proc_ctl.stop_all();
}

#[test]
fn test_gen_source() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Trace;
    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    let mut valid_source_with_probe = Media::new(0, "assets/media_mix/av_sync.mp4", true);
    let valid_media = gen_source(
        &config,
        valid_source_with_probe.clone(),
        &playout_stat,
        &play_control,
        100,
    );

    assert_eq!(valid_source_with_probe.source, valid_media.source);

    let mut valid_source_without_probe = Media::new(0, "assets/media_mix/av_sync.mp4", false);
    valid_source_without_probe.duration = 30.0;
    valid_source_without_probe.out = 20.0;
    let valid_media = gen_source(
        &config,
        valid_source_without_probe.clone(),
        &playout_stat,
        &play_control,
        100,
    );

    assert_eq!(valid_source_without_probe.source, valid_media.source);
    assert_eq!(valid_media.out, 20.0);

    valid_source_with_probe.out = 0.9;

    let valid_media = gen_source(
        &config,
        valid_source_with_probe.clone(),
        &playout_stat,
        &play_control,
        100,
    );

    assert_eq!(valid_media.out, 1.9);

    let mut no_valid_source_with_probe = Media::new(0, "assets/media_mix/av_snc.mp4", true);
    no_valid_source_with_probe.duration = 30.0;
    no_valid_source_with_probe.out = 30.0;

    let valid_media = gen_source(
        &config,
        no_valid_source_with_probe.clone(),
        &playout_stat,
        &play_control,
        100,
    );

    assert_eq!(valid_media.source, "assets/media_mix/filler.mp4");
}

#[test]
#[serial]
#[ignore]
fn playlist_missing() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2023-02-07T23:59:45");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2023-02-08");
}

#[test]
#[serial]
#[ignore]
fn playlist_next_missing() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2023-02-09T23:59:45");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2023-02-10");
}

#[test]
#[serial]
#[ignore]
fn playlist_to_short() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = log::LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2024-01-31T05:59:40");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2024-01-31");
}

#[test]
#[serial]
#[ignore]
fn playlist_init_after_list_end() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = log::LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2024-01-31T05:59:47");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2024-01-31");
}

#[test]
#[serial]
#[ignore]
fn playlist_change_at_midnight() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2023-02-08T23:59:45");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2023-02-09");
}

#[test]
#[serial]
#[ignore]
fn playlist_change_before_midnight() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "23:59:45".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Trace;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2023-02-08T23:59:30");

    thread::spawn(move || timed_stop(35, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2023-02-09");
}

#[test]
#[serial]
#[ignore]
fn playlist_change_at_six() {
    let mut config = PlayoutConfig::new(None);
    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/media_mix/playlists".into();
    config.storage.filler = "assets/media_mix/filler.mp4".into();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.out.mode = Null;
    config.out.output_count = 1;
    config.out.output_filter = None;
    config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    mock_time::set_mock_time("2023-02-09T05:59:45");

    thread::spawn(move || timed_stop(28, proc_ctl));

    player(&config, &play_control, playout_stat.clone(), proc_control);

    let playlist_date = &*playout_stat.current_date.lock().unwrap();

    assert_eq!(playlist_date, "2023-02-09");
}
