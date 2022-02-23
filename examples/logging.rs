extern crate log;
extern crate simplelog;

use simplelog::*;

use file_rotate::{FileRotate, ContentLimit, suffix::AppendCount, compression::Compression};

fn main() {
    //TermLogger::init(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto).unwrap();
    //SimpleLogger::init(LevelFilter::Debug, Config::default()).unwrap();
    // CombinedLogger::init(
    //     vec![
    //         TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
    //         WriteLogger::new(LevelFilter::Debug, Config::default(), File::create("my_rust_binary.log").unwrap()),
    //     ]
    // ).unwrap();

    let log = || {
        FileRotate::new(
            "logs/ffplayout.log",
            AppendCount::new(7),
            ContentLimit::Lines(1000),
            Compression::None,
        )
    };

    WriteLogger::init(LevelFilter::Debug, Config::default(), log()).unwrap();

    for idx in 1..1500 {
        info!("{idx}");
    }
}
