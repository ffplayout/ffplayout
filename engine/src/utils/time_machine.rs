/// These functions are made for testing purposes.
/// It allows, with a hidden command line argument, to override the time in this program.
/// It is like a time machine where you can fake the time and make the hole program think it is running in the future or past.
use std::{
    process,
    sync::{Arc, LazyLock, Mutex},
};

use chrono::{prelude::*, TimeDelta};

// Thread-local storage for time offset when mocking the time
static DATE_TIME_DIFF: LazyLock<Arc<Mutex<Option<TimeDelta>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

// Set the mock time offset if `--fake-time` argument is provided
pub fn set_mock_time(fake_time: &Option<String>) {
    if let Some(time) = fake_time {
        if let Ok(mock_time) = DateTime::parse_from_rfc3339(time) {
            let mock_time = mock_time.with_timezone(&Local);
            // Calculate the offset from the real current time
            let mut diff = DATE_TIME_DIFF.lock().unwrap();
            *diff = Some(Local::now() - mock_time);
        } else {
            eprintln!(
                "Error: Invalid date format for --fake-time, use time with offset in: 2024-10-27T00:59:00+02:00"
            );
            process::exit(1);
        }
    }
}

// Function to get the current time, using either real or mock time based on `--fake-time`
pub fn time_now() -> DateTime<Local> {
    let diff = DATE_TIME_DIFF.lock().unwrap();
    if let Some(d) = &*diff {
        // If `--fake-time` is set, use the offset time
        Local::now() - *d
    } else {
        // Otherwise, use the real current time
        Local::now()
    }
}
