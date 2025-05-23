/// These functions are made for testing purposes.
/// It allows, with a hidden command line argument, to override the time in this program.
/// It is like a time machine where you can fake the time and make the hole program think it is running in the future or past.
use std::{
    io,
    str::FromStr,
    sync::{Arc, LazyLock, RwLock},
};

use chrono::{TimeDelta, prelude::*};
use chrono_tz::Tz;

// Thread-local storage for time offset when mocking the time
static DATE_TIME_DIFF: LazyLock<Arc<RwLock<Option<TimeDelta>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Set the mock time offset if `--fake-time` argument is provided
pub fn set_mock_time(fake_time: &Option<String>) -> Result<(), io::Error> {
    if let Some(time) = fake_time {
        match DateTime::parse_from_rfc3339(time) {
            Ok(mock_time) => {
                let mock_time: DateTime<Utc> = mock_time.into();
                // Calculate the offset from the real current time
                let mut diff = DATE_TIME_DIFF.write().unwrap();
                *diff = Some(Utc::now() - mock_time);
            }
            Err(..) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Error: Invalid date format for --fake-time, use time with offset in: 2024-10-27T00:59:00+02:00",
                ));
            }
        }
    }

    Ok(())
}

// Function to get the current time, using either real or mock time based on `--fake-time`
pub fn time_now(timezone: &Option<Tz>) -> DateTime<Tz> {
    let utc_now: DateTime<Utc> = Utc::now();

    let tz = match timezone {
        Some(tz) => *tz,
        None => match iana_time_zone::get_timezone()
            .ok()
            .and_then(|t: String| Tz::from_str(&t).ok())
        {
            Some(tz) => tz,
            None => Tz::UTC,
        },
    };

    match DATE_TIME_DIFF.read().ok().and_then(|d| *d) {
        Some(d) => utc_now.with_timezone(&tz) - d,
        None => utc_now.with_timezone(&tz),
    }
}
