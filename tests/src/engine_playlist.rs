use std::{
    thread::{self, sleep},
    time::Duration,
};

use simplelog::*;

use ffplayout::output::player;
use ffplayout_lib::utils::*;

fn timed_kill(sec: u64, mut proc_ctl: ProcessControl) {
    sleep(Duration::from_secs(sec));

    proc_ctl.kill_all();
}

#[test]
#[ignore]
fn playlist_change_at_midnight() {
    let mut config = PlayoutConfig::new(None);
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.length = "24:00:00".into();
    config.logging.log_to_file = false;

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    mock_time::set_mock_time("2022-05-09T23:59:45");

    thread::spawn(move || timed_kill(30, proc_ctl));

    player(&config, play_control, playout_stat, proc_control);
}

#[test]
#[ignore]
fn playlist_change_at_six() {
    let mut config = PlayoutConfig::new(None);
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.length = "24:00:00".into();
    config.logging.log_to_file = false;

    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let proc_ctl = proc_control.clone();

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    mock_time::set_mock_time("2022-05-09T05:59:45");

    thread::spawn(move || timed_kill(30, proc_ctl));

    player(&config, play_control, playout_stat, proc_control);
}
