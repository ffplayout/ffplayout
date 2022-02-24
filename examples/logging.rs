extern crate log;
extern crate simplelog;

use simplelog::*;

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};

fn main() {
    let log = || {
        FileRotate::new(
            "logs/ffplayout.log",
            AppendCount::new(7),
            ContentLimit::Lines(1000),
            Compression::None,
        )
    };

    let def_config = simplelog::ConfigBuilder::new()
        .set_target_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Left)
        .set_time_to_local(true).clone();

    let term_config = def_config.clone()
        .set_level_color(Level::Debug, Some(Color::Ansi256(12)))
        .set_level_color(Level::Info, Some(Color::Ansi256(10)))
        .set_level_color(Level::Warn, Some(Color::Ansi256(208)))
        .set_level_color(Level::Error, Some(Color::Ansi256(9)))
        .set_time_format_str("\x1b[30;1m[%Y-%m-%d %H:%M:%S%.3f]\x1b[0m")
        .build();

    let file_config = def_config.clone()
        .set_time_format_str("[%Y-%m-%d %H:%M:%S%.3f]")
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            term_config,
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
        WriteLogger::new(LevelFilter::Debug, file_config, log()),
    ])
    .unwrap();

    debug!("this is a <b>debug</> message");
    info!("this is a info message");
    warn!("this is a warning message");
    error!("this is a error message");

    for idx in 1..10 {
        info!("{idx}");
    }
}
