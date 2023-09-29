use std::path::PathBuf;

use clap::Parser;

use ffplayout_lib::utils::{OutputMode, ProcessMode};

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
    pub config: Option<PathBuf>,

    #[clap(short, long, help = "File path for logging")]
    pub log: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Target date (YYYY-MM-DD) for text/m3u to playlist import"
    )]
    pub date: Option<String>,

    #[cfg(debug_assertions)]
    #[clap(long, help = "fake date time, for debugging")]
    pub fake_time: Option<String>,

    #[clap(short, long, help = "Play folder content")]
    pub folder: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Generate playlist for dates, like: 2022-01-01 - 2022-01-10",
        name = "YYYY-MM-DD",
        num_args = 1..,
    )]
    pub generate: Option<Vec<String>>,

    #[clap(
        long,
        help = "Import a given text/m3u file and create a playlist from it"
    )]
    pub import: Option<PathBuf>,

    #[clap(short, long, help = "Loop playlist infinitely")]
    pub infinit: bool,

    #[clap(
        short = 't',
        long,
        help = "Set length in 'hh:mm:ss', 'none' for no length check"
    )]
    pub length: Option<String>,

    #[clap(long, help = "Override logging level")]
    pub level: Option<String>,

    #[clap(long, help = "Optional path list for playlist generations", num_args = 1..)]
    pub paths: Option<Vec<PathBuf>>,

    #[clap(short = 'm', long, help = "Playing mode: folder, playlist")]
    pub play_mode: Option<ProcessMode>,

    #[clap(short, long, help = "Path to playlist, or playlist root folder.")]
    pub playlist: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Start time in 'hh:mm:ss', 'now' for start with first"
    )]
    pub start: Option<String>,

    #[clap(short = 'T', long, help = "JSON Template file for generating playlist")]
    pub template: Option<PathBuf>,

    #[clap(short, long, help = "Set output mode: desktop, hls, null, stream")]
    pub output: Option<OutputMode>,

    #[clap(short, long, help = "Set audio volume")]
    pub volume: Option<f64>,

    #[clap(long, help = "validate given playlist")]
    pub validate: bool,
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
