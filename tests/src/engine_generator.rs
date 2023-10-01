use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::NaiveTime;
use simplelog::*;

use ffplayout_lib::utils::{
    config::{Source, Template},
    generator::*,
    *,
};

#[test]
fn test_random_list() {
    let clip_list = vec![
        Media::new(0, "./assets/with_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/dual_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/av_sync.mp4", true),    // 30 seconds
        Media::new(0, "./assets/ad.mp4", true),         // 25 seconds
    ];

    let r_list = random_list(clip_list.clone(), 200.0);
    let r_duration = sum_durations(&r_list);

    assert!(200.0 >= r_duration, "duration is {r_duration}");
    assert!(r_duration >= 170.0);
}

#[test]
#[ignore]
fn test_ordered_list() {
    let clip_list = vec![
        Media::new(0, "./assets/with_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/dual_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/av_sync.mp4", true),    // 30 seconds
        Media::new(0, "./assets/ad.mp4", true),         // 25 seconds
    ];

    let o_list = ordered_list(clip_list.clone(), 85.0);

    assert_eq!(o_list.len(), 3);
    assert_eq!(o_list[2].duration, 25.0);
    assert_eq!(sum_durations(&o_list), 85.0);

    let o_list = ordered_list(clip_list, 120.0);

    assert_eq!(o_list.len(), 4);
    assert_eq!(o_list[2].duration, 30.0);
    assert_eq!(sum_durations(&o_list), 115.0);
}

#[test]
#[ignore]
fn test_filler_list() {
    let mut config = PlayoutConfig::new(None);
    config.storage.filler = "assets/".into();

    let f_list = filler_list(&config, 2440.0);

    assert_eq!(sum_durations(&f_list), 2440.0);
}

#[test]
#[ignore]
fn test_generate_playlist_from_folder() {
    let mut config = PlayoutConfig::new(None);
    config.general.generate = Some(vec!["2023-09-11".to_string()]);
    config.processing.mode = Playlist;
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Error;
    config.storage.filler = "assets/".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/playlists".into();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    let playlist = generate_playlist(&config, Some("Channel 1".to_string()));

    assert!(playlist.is_ok());

    let playlist_file = Path::new("assets/playlists/2023/09/2023-09-11.json");

    assert!(playlist_file.is_file());

    fs::remove_file(playlist_file).unwrap();

    let total_duration = sum_durations(&playlist.unwrap()[0].program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );
}

#[test]
#[ignore]
fn test_generate_playlist_from_template() {
    let mut config = PlayoutConfig::new(None);
    config.general.generate = Some(vec!["2023-09-12".to_string()]);
    config.general.template = Some(Template {
        sources: vec![
            Source {
                start: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: false,
                paths: vec![PathBuf::from("assets/")],
            },
            Source {
                start: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: true,
                paths: vec![PathBuf::from("assets/")],
            },
        ],
    });
    config.processing.mode = Playlist;
    config.logging.log_to_file = false;
    config.logging.timestamp = false;
    config.logging.level = LevelFilter::Error;
    config.storage.filler = "assets/".into();
    config.playlist.length_sec = Some(86400.0);
    config.playlist.path = "assets/playlists".into();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap_or_default();

    let playlist = generate_playlist(&config, Some("Channel 1".to_string()));

    assert!(playlist.is_ok());

    let playlist_file = Path::new("assets/playlists/2023/09/2023-09-12.json");

    assert!(playlist_file.is_file());

    fs::remove_file(playlist_file).unwrap();

    let total_duration = sum_durations(&playlist.unwrap()[0].program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );
}
