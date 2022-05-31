#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use crate::utils::*;
use crate::vec_strings;

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

    let date = get_date(true, 21600.0, 86400.0);

    assert_eq!("2022-05-19".to_string(), date);
}

#[test]
fn get_date_tomorrow() {
    mock_time::set_mock_time("2022-05-20T23:59:30");

    let date = get_date(false, 0.0, 86400.01);

    assert_eq!("2022-05-21".to_string(), date);
}

#[test]
fn test_delta() {
    let mut config = GlobalConfig::new();
    config.mail.recipient = "".into();
    config.processing.mode = "playlist".into();
    config.playlist.day_start = "00:00:00".into();
    config.playlist.length = "24:00:00".into();
    config.logging.log_to_file = false;

    mock_time::set_mock_time("2022-05-09T23:59:59");
    let (delta, _) = get_delta(&config, &86401.0);

    assert!(delta < 2.0);
}

#[test]
fn test_prepare_output_cmd() {
    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];
    let filter = vec_strings![
        "-filter_complex",
        "[0:v]null,zmq=b=tcp\\\\://'127.0.0.1\\:5555',drawtext=text=''"
    ];
    let params = vec_strings![
        "-c:v",
        "libx264",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost:1937/live/stream"
    ];

    let mut t1_params = enc_prefix.clone();
    t1_params.append(&mut params.clone());
    let cmd_two_outs =
        prepare_output_cmd(enc_prefix.clone(), vec_strings![], params.clone(), "stream");

    assert_eq!(cmd_two_outs, t1_params);

    let mut test_cmd = enc_prefix.clone();
    let mut test_params = params.clone();
    let mut t2_filter = filter.clone();
    t2_filter[1].push_str(",split=2[v_out1][v_out2]");
    test_cmd.append(&mut t2_filter);

    test_params.insert(0, "-map".to_string());
    test_params.insert(1, "[v_out1]".to_string());
    test_params.insert(2, "-map".to_string());
    test_params.insert(3, "0:a".to_string());

    test_params.insert(11, "-map".to_string());
    test_params.insert(12, "[v_out2]".to_string());
    test_params.insert(13, "-map".to_string());
    test_params.insert(14, "0:a".to_string());

    test_cmd.append(&mut test_params);
    let cmd_two_outs_with_filter = prepare_output_cmd(enc_prefix, filter, params, "stream");

    assert_eq!(cmd_two_outs_with_filter, test_cmd);
}
