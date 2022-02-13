use chrono::prelude::*;

pub fn get_sec() -> f64 {
    let local: DateTime<Local> = Local::now();

    let sec = (
        local.hour() * 3600 + local.minute() * 60 + local.second()
    ) as f64 + (local.nanosecond() as f64 / 1000000000.0);

    sec
}

pub fn get_timestamp() -> i64 {
    let local: DateTime<Local> = Local::now();

    local.timestamp_millis() as i64
}
