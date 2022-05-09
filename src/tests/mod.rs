#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use crate::utils::*;

#[cfg(test)]
fn get_fake_date_time(date_time: &str) -> DateTime<Local> {
    let date_obj = NaiveDateTime::parse_from_str(date_time, "%Y-%m-%dT%H:%M:%S");

    Local.from_local_datetime(&date_obj.unwrap()).unwrap()
}

#[test]
fn mock_date_time() {
    let fake_time = get_fake_date_time("2022-05-20T06:00:00");
    mock_time::set_mock_time(fake_time);

    assert_eq!(
        fake_time.format("%Y-%m-%dT%H:%M:%S.2f").to_string(),
        time_now().format("%Y-%m-%dT%H:%M:%S.2f").to_string()
    );
}

#[test]
fn get_date_yesterday() {
    let fake_time = get_fake_date_time("2022-05-20T05:59:24");
    mock_time::set_mock_time(fake_time);

    let date = get_date(true, 21600.0, 86400.0);

    assert_eq!("2022-05-19".to_string(), date);
}

#[test]
fn get_date_tomorrow() {
    let fake_time = get_fake_date_time("2022-05-20T23:59:30");
    mock_time::set_mock_time(fake_time);

    let date = get_date(false, 0.0, 86400.01);

    assert_eq!("2022-05-21".to_string(), date);
}
