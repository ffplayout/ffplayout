use std::{
    io,
    sync::{Arc, LazyLock, RwLock},
    thread::sleep,
    time::Duration,
};

use chrono::{TimeDelta, prelude::*};
use chrono_tz::Tz;
use clap::Parser;

// Struct to hold command-line arguments
#[derive(Parser, Debug, Clone)]
#[clap(version, about = "run time machine")]
struct Args {
    #[clap(short, long, help = "set time")]
    fake_time: Option<String>,
}

static DATE_TIME_DIFF: LazyLock<Arc<RwLock<Option<TimeDelta>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Thread-local storage for time offset when mocking the time
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
pub fn time_now(timezone: Option<Tz>) -> DateTime<Tz> {
    let utc_now: DateTime<Utc> = Utc::now();

    let tz = match timezone {
        Some(tz) => tz,
        None => Tz::UTC,
    };

    match DATE_TIME_DIFF.read().ok().and_then(|d| *d) {
        Some(d) => utc_now.with_timezone(&tz) - d,
        None => utc_now.with_timezone(&tz),
    }
}

fn main() {
    let tz = Some(chrono_tz::Europe::Berlin);
    let args = Args::parse();

    // Initialize mock time if `--fake-time` is set
    set_mock_time(&args.fake_time).unwrap();

    loop {
        println!("Current time (or mocked time): {}", time_now(tz));

        sleep(Duration::from_secs(1));
    }
}

#[test]
fn get_time() {
    let tz = Some(chrono_tz::Europe::Berlin);
    println!("{}", time_now(tz));
}
