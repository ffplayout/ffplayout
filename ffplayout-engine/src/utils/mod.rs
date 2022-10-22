use std::{
    path::{Path, PathBuf},
    process::exit,
};

use regex::Regex;

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    filter::Filters,
    utils::{time_to_sec, PlayoutConfig, ProcessMode::*},
};

/// Read command line arguments, and override the config with them.
pub fn get_config(args: Args) -> PlayoutConfig {
    let cfg_path = match args.channel {
        Some(c) => {
            let path = PathBuf::from(format!("/etc/ffplayout/{c}.yml"));

            if !path.is_file() {
                println!(
                    "Config file \"{c}\" under \"/etc/ffplayout/\" not found.\n\nCheck arguments!"
                );
                exit(1)
            }

            Some(path.display().to_string())
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
        config.processing.mode = Folder;
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

/// Prepare output parameters
///
/// Seek for multiple outputs and add mapping for it.
pub fn prepare_output_cmd(
    config: &PlayoutConfig,
    mut cmd: Vec<String>,
    filters: &Option<Filters>,
) -> Vec<String> {
    let mut output_params = config.out.clone().output_cmd.unwrap();
    let mut new_params = vec![];
    let mut count = 1;
    let re_map = Regex::new(r"(\[?[0-9]:[av](:[0-9]+)?\]?|-map$|\[[a-z_0-9]+\])").unwrap(); // match a/v filter links and mapping

    if let Some(mut filter) = filters.clone() {
        println!("filter: {filter:#?}\n");
        for (i, param) in output_params.iter().enumerate() {
            if !re_map.is_match(param)
                || (i < output_params.len() - 2
                    && (output_params[i + 1].contains("0:s") || param.contains("0:s")))
            {
                // Skip mapping parameters, when no multi in/out filter is set.
                // Only add subtitle mapping.
                new_params.push(param.clone());
            }

            // Check if parameter is a output
            if i > 0
                && !param.starts_with('-')
                && !output_params[i - 1].starts_with('-')
                && i < output_params.len() - 1
            {
                // add mapping to following outputs
                new_params.append(&mut filter.map(Some(count)));

                count += 1
            }
        }

        output_params = new_params;

        cmd.append(&mut filter.cmd());

        if config.out.output_count > 1 {
            cmd.append(&mut filter.map(Some(0)));
        } else {
            cmd.append(&mut filter.map(None));
        }
    }

    cmd.append(&mut output_params);

    cmd
}
