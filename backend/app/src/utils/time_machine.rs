/// These functions are made for testing purposes.
/// It allows, with a hidden command line argument, to override the time in this program.
/// It is like a time machine where you can fake the time and make the hole program think it is running in the future or past.
use std::{io, str::FromStr};

#[cfg(test)]
use std::cell::Cell;
#[cfg(not(test))]
use std::sync::{Arc, LazyLock, RwLock};

use chrono::{TimeDelta, prelude::*};
use chrono_tz::Tz;

// Application-wide offset used by --fake-time.
#[cfg(not(test))]
static DATE_TIME_DIFF: LazyLock<Arc<RwLock<Option<TimeDelta>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Unit tests use synchronous, scoped clocks so parallel tests cannot interfere.
#[cfg(test)]
thread_local! {
    static DATE_TIME_DIFF: Cell<Option<TimeDelta>> = const { Cell::new(None) };
}

// Set the mock time offset if `--fake-time` argument is provided
pub fn set_mock_time(fake_time: &Option<String>) -> Result<(), io::Error> {
    if let Some(time) = fake_time {
        match DateTime::parse_from_rfc3339(time) {
            Ok(mock_time) => {
                let mock_time: DateTime<Utc> = mock_time.into();
                // Calculate the offset from the real current time
                let offset = Some(Utc::now() - mock_time);
                #[cfg(not(test))]
                {
                    *DATE_TIME_DIFF.write().unwrap() = offset;
                }
                #[cfg(test)]
                DATE_TIME_DIFF.set(offset);
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

    #[cfg(not(test))]
    let offset = DATE_TIME_DIFF.read().ok().and_then(|d| *d);
    #[cfg(test)]
    let offset = DATE_TIME_DIFF.get();

    match offset {
        Some(d) => utc_now.with_timezone(&tz) - d,
        None => utc_now.with_timezone(&tz),
    }
}

#[cfg(test)]
pub(crate) fn with_mock_time<T>(time: &str, test: impl FnOnce() -> T) -> T {
    struct ResetClock(Option<TimeDelta>);

    impl Drop for ResetClock {
        fn drop(&mut self) {
            DATE_TIME_DIFF.set(self.0);
        }
    }

    let _reset = ResetClock(DATE_TIME_DIFF.get());
    set_mock_time(&Some(time.to_string())).unwrap();

    test()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_date_time_uses_an_explicit_timezone_and_restores_the_clock() {
        let before = DATE_TIME_DIFF.get();
        with_mock_time("2022-05-20T06:00:00+02:00", || {
            assert_eq!(
                time_now(&Some(Tz::Europe__Berlin))
                    .format("%FT%T%:z")
                    .to_string(),
                "2022-05-20T06:00:00+02:00"
            );
        });

        assert_eq!(DATE_TIME_DIFF.get(), before);
    }
}
