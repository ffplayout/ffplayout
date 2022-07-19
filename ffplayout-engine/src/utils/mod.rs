use std::path::{Path, PathBuf};

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::utils::{time_to_sec, PlayoutConfig};

pub fn get_config(args: Args) -> PlayoutConfig {
    let cfg_path = match args.channel {
        Some(c) => {
            let path = PathBuf::from(format!("/etc/ffplayout/{c}.yml"));

            if path.is_file() {
                Some(path.display().to_string())
            } else {
                println!("no file");
                args.config
            }
        }
        None => args.config,
    };
    let mut config = PlayoutConfig::new(cfg_path);

    if let Some(gen) = args.generate {
        config.general.generate = Some(gen);
    }

    if let Some(log_path) = args.log {
        if Path::new(&log_path).is_dir() {
            config.logging.log_to_file = true;
        }
        config.logging.log_path = log_path;
    }

    if let Some(playlist) = args.playlist {
        config.playlist.path = playlist;
    }

    if let Some(mode) = args.play_mode {
        config.processing.mode = mode;
    }

    if let Some(folder) = args.folder {
        config.storage.path = folder;
        config.processing.mode = "folder".into();
    }

    if let Some(start) = args.start {
        config.playlist.day_start = start.clone();
        config.playlist.start_sec = Some(time_to_sec(&start));
    }

    if let Some(length) = args.length {
        config.playlist.length = length.clone();

        if length.contains(':') {
            config.playlist.length_sec = Some(time_to_sec(&length));
        } else {
            config.playlist.length_sec = Some(86400.0);
        }
    }

    if args.infinit {
        config.playlist.infinit = args.infinit;
    }

    if let Some(output) = args.output {
        config.out.mode = output;
    }

    if let Some(volume) = args.volume {
        config.processing.volume = volume;
    }

    config
}
// Read command line arguments, and override the config with them.
