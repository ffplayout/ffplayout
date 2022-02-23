extern crate log;
extern crate simplelog;

use simplelog::*;

use std::fs::File;

fn main() {
    TermLogger::init(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto).unwrap();
    //SimpleLogger::init(LevelFilter::Debug, Config::default()).unwrap();
    // CombinedLogger::init(
    //     vec![
    //         TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
    //         WriteLogger::new(LevelFilter::Debug, Config::default(), File::create("my_rust_binary.log").unwrap()),
    //     ]
    // ).unwrap();

    error!("Bright red error");
    info!("This only appears in <red>the log file</>");
    debug!("This level is <b>currently</b> not enabled for any logger");
}
