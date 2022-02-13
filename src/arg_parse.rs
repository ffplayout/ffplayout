use clap::Parser;

#[derive(Parser, Debug)]
#[clap(version, about = "ffplayout, the rust playout solution", long_about = None)]
pub struct Args {
    #[clap(short, long, help = "file path to ffplayout.conf")]
    pub config: Option<String>,

    #[clap(short, long, help = "play folder content")]
    pub folder:  Option<String>,

    #[clap(short, long, help = "file path for logfile")]
    pub log:  Option<String>,

    #[clap(short = 'i', long, help = "loop playlist infinitely")]
    pub r#loop: bool,

    #[clap(short, long,
        help = "set output mode: desktop, hls, stream")]
    pub output:  Option<String>,

    #[clap(short, long, help = "path from playlist")]
    pub playlist:  Option<String>,

    #[clap(short, long, help = "start time in 'hh:mm:ss', 'now' for start with first")]
    pub start:  Option<String>,

    #[clap(short = 't', long, help = "set length in 'hh:mm:ss', 'none' for no length check")]
    pub length:  Option<String>,

    #[clap(long, help = "playing mode: folder, playlist, custom...")]
    pub play_mode:  Option<String>,
}

pub fn get_args() -> Args {
    let args = Args::parse();

    args
}
