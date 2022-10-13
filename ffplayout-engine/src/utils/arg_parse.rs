use clap::Parser;

use ffplayout_lib::utils::OutputMode;

#[derive(Parser, Debug, Clone)]
#[clap(version,
    about = "ffplayout, Rust based 24/7 playout solution.",
    override_usage = "Run without any command to use config file only, or with commands to override parameters:
                      \n    ffplayout (ARGS) [OPTIONS]\n\n    Pass channel name only in multi channel environment!",
    long_about = None)]
pub struct Args {
    #[clap(index = 1, value_parser, help = "Channel name")]
    pub channel: Option<String>,

    #[clap(short, long, help = "File path to ffplayout.yml")]
    pub config: Option<String>,

    #[clap(short, long, help = "File path for logging")]
    pub log: Option<String>,

    #[clap(
        short,
        long,
        help = "Generate playlist for dates, like: 2022-01-01 - 2022-01-10",
        name = "YYYY-MM-DD",
        multiple_values = true
    )]
    pub generate: Option<Vec<String>>,

    #[clap(short = 'm', long, help = "Playing mode: folder, playlist")]
    pub play_mode: Option<String>,

    #[clap(short, long, help = "Play folder content")]
    pub folder: Option<String>,

    #[clap(
        short,
        long,
        help = "Target date (YYYY-MM-DD) for text/m3u to playlist import"
    )]
    pub date: Option<String>,

    #[clap(
        long,
        help = "Import a given text/m3u file and create a playlist from it"
    )]
    pub import: Option<String>,

    #[clap(short, long, help = "Path from playlist")]
    pub playlist: Option<String>,

    #[clap(
        short,
        long,
        help = "Start time in 'hh:mm:ss', 'now' for start with first"
    )]
    pub start: Option<String>,

    #[clap(
        short = 't',
        long,
        help = "Set length in 'hh:mm:ss', 'none' for no length check"
    )]
    pub length: Option<String>,

    #[clap(short, long, help = "Loop playlist infinitely")]
    pub infinit: bool,

    #[clap(short, long, help = "Set output mode: desktop, hls, null, stream")]
    pub output: Option<OutputMode>,

    #[clap(short, long, help = "Set audio volume")]
    pub volume: Option<f64>,

    #[cfg(debug_assertions)]
    #[clap(long, help = "fake date time, for debugging")]
    pub fake_time: Option<String>,
}

/// Get arguments from command line, and return them.
#[cfg(not(test))]
pub fn get_args() -> Args {
    Args::parse()
}

#[cfg(test)]
pub fn get_args() -> Args {
    Args::parse_from(["-o desktop"].iter())
}
