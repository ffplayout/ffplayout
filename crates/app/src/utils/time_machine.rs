/// These functions are made for testing purposes.
/// It allows, with a hidden command line argument, to override the time in this program.
/// It is like a time machine where you can fake the time and make the hole program think it is running in the future or past.
use std::io;

#[cfg(test)]
use std::cell::Cell;
#[cfg(not(test))]
use std::sync::{Arc, LazyLock, RwLock};

use jiff::{SignedDuration, Timestamp, ZonedDateTime, tz::TimeZone};
use serde::{Deserialize, Deserializer, de};

// Application-wide offset used by --fake-time.
#[cfg(not(test))]
static DATE_TIME_DIFF: LazyLock<Arc<RwLock<Option<SignedDuration>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Unit tests use synchronous, scoped clocks so parallel tests cannot interfere.
#[cfg(test)]
thread_local! {
    static DATE_TIME_DIFF: Cell<Option<SignedDuration>> = const { Cell::new(None) };
}

// Set the mock time offset if `--fake-time` argument is provided
pub fn set_mock_time(fake_time: &Option<String>) -> Result<(), io::Error> {
    if let Some(time) = fake_time {
        match time.parse::<Timestamp>() {
            Ok(mock_time) => {
                // Calculate the offset from the real current time
                let offset = Some(Timestamp::now().duration_since(mock_time));
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

pub(crate) fn parse_channel_timezone(name: &str) -> Option<TimeZone> {
    TimeZone::get(name)
        .ok()
        .filter(|tz| tz.iana_name().is_some())
}

pub(crate) fn deserialize_channel_timezone<'de, D>(
    deserializer: D,
) -> Result<Option<TimeZone>, D::Error>
where
    D: Deserializer<'de>,
{
    let name = Option::<String>::deserialize(deserializer)?;

    name.map(|name| {
        parse_channel_timezone(&name)
            .ok_or_else(|| de::Error::custom(format!("Invalid IANA time zone: {name}")))
    })
    .transpose()
}

pub(crate) fn default_channel_timezone() -> TimeZone {
    TimeZone::try_system()
        .ok()
        .filter(|tz| tz.iana_name().is_some())
        .unwrap_or(TimeZone::UTC)
}

// Function to get the current time, using either real or mock time based on `--fake-time`
pub fn time_now(timezone: &Option<TimeZone>) -> ZonedDateTime {
    let utc_now = Timestamp::now();

    let tz = timezone.clone().unwrap_or_else(default_channel_timezone);

    #[cfg(not(test))]
    let offset = DATE_TIME_DIFF.read().ok().and_then(|d| *d);
    #[cfg(test)]
    let offset = DATE_TIME_DIFF.get();

    let local = utc_now.to_zoned_datetime(tz);

    match offset {
        Some(d) => local.saturating_sub(d),
        None => local,
    }
}

#[cfg(test)]
pub(crate) fn with_mock_time<T>(time: &str, test: impl FnOnce() -> T) -> T {
    struct ResetClock(Option<SignedDuration>);

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
                time_now(&Some(TimeZone::get("Europe/Berlin").unwrap()))
                    .strftime("%FT%T%:z")
                    .to_string(),
                "2022-05-20T06:00:00+02:00"
            );
        });

        assert_eq!(DATE_TIME_DIFF.get(), before);
    }
}
