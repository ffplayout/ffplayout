use std::path::PathBuf;

#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use ffplayout_lib::utils::*;

#[test]
fn mock_date_time() {
    let time_str = "2022-05-20T06:00:00";
    let date_obj = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S");
    let time = Local.from_local_datetime(&date_obj.unwrap()).unwrap();

    mock_time::set_mock_time(time_str);

    assert_eq!(
        time.format("%Y-%m-%dT%H:%M:%S.2f").to_string(),
        time_now().format("%Y-%m-%dT%H:%M:%S.2f").to_string()
    );
}

#[test]
fn get_date_yesterday() {
    mock_time::set_mock_time("2022-05-20T05:59:24");

    let date = get_date(true, 21600.0, false);

    assert_eq!("2022-05-19".to_string(), date);
}

#[test]
fn get_date_tomorrow() {
    mock_time::set_mock_time("2022-05-20T23:59:58");

    let date = get_date(false, 0.0, true);

    assert_eq!("2022-05-21".to_string(), date);
}

#[test]
fn test_delta() {
    let mut config = PlayoutConfig::new(Some(PathBuf::from("../assets/ffplayout.yml")), None);
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.length = "24:00:00".into();
    config.logging.log_to_file = false;

    mock_time::set_mock_time("2022-05-09T23:59:59");
    let (delta, _) = get_delta(&config, &86401.0);

    assert!(delta < 2.0);
}
