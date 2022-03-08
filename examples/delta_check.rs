use chrono::prelude::*;
use std::{time, time::UNIX_EPOCH};

pub fn get_sec() -> f64 {
    let local: DateTime<Local> = Local::now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

pub fn sec_to_time(sec: f64) -> String {
    let d = UNIX_EPOCH + time::Duration::from_secs(sec as u64);
    // Create DateTime from SystemTime
    let date_time = DateTime::<Utc>::from(d);

    date_time.format("%H:%M:%S").to_string()
}

fn main() {
    let current_time = get_sec();
    let start = 21600.0;
    let target_length = 86400.0;
    let total_delta;

    if current_time < start {
        total_delta = start - current_time;
    } else {
        total_delta = target_length + start - current_time;
    }

    println!("Total Seconds: {total_delta}");
    println!("Total Time:    {}", sec_to_time(total_delta));
}
