use std::thread::sleep;
use std::time::Duration;

use ffplayout_api::utils::system;
use ffplayout_lib::utils::PlayoutConfig;

fn main() {
    loop {
        let system_stat = system::stat(PlayoutConfig::default());
        print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
        println!("{system_stat:#?}");

        sleep(Duration::from_secs(1))
    }
}
