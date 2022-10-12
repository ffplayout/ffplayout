use std::{
    path::{Path, PathBuf},
    process::exit,
};

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    utils::{time_to_sec, PlayoutConfig},
    vec_strings,
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

/// Prepare output parameters
///
/// seek for multiple outputs and add mapping for it
pub fn prepare_output_cmd(
    prefix: Vec<String>,
    mut filter: Vec<String>,
    params: Vec<String>,
    mode: &str,
) -> Vec<String> {
    let params_len = params.len();
    let mut output_params = params.clone();
    let mut output_a_map = "[a_out1]".to_string();
    let mut output_v_map = "[v_out1]".to_string();
    let mut output_count = 1;
    let mut cmd = prefix;

    if !filter.is_empty() {
        output_params.clear();

        for (i, p) in params.iter().enumerate() {
            let mut param = p.clone();

            param = param.replace("[0:v]", "[vout0]");
            param = param.replace("[0:a]", "[aout0]");

            if param != "-filter_complex" {
                output_params.push(param.clone());
            }

            if i > 0
                && !param.starts_with('-')
                && !params[i - 1].starts_with('-')
                && i < params_len - 1
            {
                output_count += 1;
                let mut a_map = "0:a".to_string();
                let v_map = format!("[v_out{output_count}]");
                output_v_map.push_str(&v_map);

                if mode == "hls" {
                    a_map = format!("[a_out{output_count}]");
                }

                output_a_map.push_str(&a_map);

                let mut map = vec_strings!["-map", v_map, "-map", a_map];

                output_params.append(&mut map);
            }
        }

        if output_count > 1 && mode == "hls" {
            filter[1].push_str(&format!(";[vout0]split={output_count}{output_v_map}"));
            filter[1].push_str(&format!(";[aout0]asplit={output_count}{output_a_map}"));
            filter.drain(2..);
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "[a_out1]"]);
        } else if output_count == 1 && mode == "hls" && output_params[0].contains("split") {
            let out_filter = output_params.remove(0);
            filter[1].push_str(&format!(";{out_filter}"));
            filter.drain(2..);
            cmd.append(&mut filter);
        } else if output_count > 1 && mode == "stream" {
            filter[1].push_str(&format!(",split={output_count}{output_v_map}"));
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "0:a"]);
        } else {
            cmd.append(&mut filter);
        }
    }

    cmd.append(&mut output_params);

    cmd
}
