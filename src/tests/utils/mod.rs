#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use crate::utils::*;

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
