use std::{thread::sleep, time::Duration};

use clap::Parser;
use jiff::tz::TimeZone;

use ffplayout::utils::time_machine::{set_mock_time, time_now};

// Struct to hold command-line arguments
#[derive(Parser, Debug, Clone)]
#[clap(version, about = "run time machine")]
struct Args {
    #[clap(short, long, help = "set time")]
    fake_time: Option<String>,
}

fn main() {
    let tz = Some(TimeZone::get("Europe/Berlin").unwrap());
    let args = Args::parse();

    // Initialize mock time if `--fake-time` is set
    set_mock_time(&args.fake_time).unwrap();

    loop {
        println!("Current time (or mocked time): {}", time_now(&tz));

        sleep(Duration::from_secs(1));
    }
}

#[test]
fn get_time() {
    let tz = Some(TimeZone::get("Europe/Berlin").unwrap());
    println!("{}", time_now(&tz));
}
