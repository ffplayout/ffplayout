use chrono::prelude::*;
use chrono::Duration;
use std::fs::metadata;

pub fn get_sec() -> f64 {
    let local: DateTime<Local> = Local::now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()
    ) as f64 + (local.nanosecond() as f64 / 1000000000.0)
}

// pub fn get_timestamp() -> i64 {
//     let local: DateTime<Local> = Local::now();

//     local.timestamp_millis() as i64
// }

pub fn get_date(seek: bool, start: f64, next: f64) -> String {
    let local: DateTime<Local> = Local::now();

    if seek && start > get_sec() {
        return (local - Duration::days(1)).format("%Y-%m-%d").to_string()
    }

    if start == 0.0 && next >= 86400.0 {
        return (local + Duration::days(1)).format("%Y-%m-%d").to_string()
    }

    local.format("%Y-%m-%d").to_string()
}

pub fn modified_time(path: String) -> Option<DateTime<Local>> {
    let metadata = metadata(path).unwrap();

    if let Ok(time) = metadata.modified() {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time)
    }

    None
}
