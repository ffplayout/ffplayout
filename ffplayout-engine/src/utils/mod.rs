use std::{
    fs::File,
    path::{Path, PathBuf},
};

use regex::Regex;
use serde_json::{json, Map, Value};
use simplelog::*;

pub mod arg_parse;
pub mod task_runner;

pub use arg_parse::Args;
use ffplayout_lib::{
    filter::Filters,
    utils::{
        config::Template, errors::ProcError, parse_log_level_filter, sec_to_time, time_in_seconds,
        time_to_sec, Media, OutputMode::*, PlayoutConfig, PlayoutStatus, ProcessMode::*,
    },
    vec_strings,
};

/// Read command line arguments, and override the config with them.
pub fn get_config(args: Args) -> Result<PlayoutConfig, ProcError> {
    let cfg_path = match args.channel {
        Some(c) => {
            let path = PathBuf::from(format!("/etc/ffplayout/{c}.yml"));

            if !path.is_file() {
                return Err(ProcError::Custom(format!(
                    "Config file \"{c}\" under \"/etc/ffplayout/\" not found.\n\nCheck arguments!"
                )));
            }

            Some(path)
        }
        None => args.config,
    };

    let mut config = PlayoutConfig::new(cfg_path);

    if let Some(gen) = args.generate {
        config.general.generate = Some(gen);
    }

    if args.validate {
        config.general.validate = true;
    }

    if let Some(template_file) = args.template {
        let f = File::options()
            .read(true)
            .write(false)
            .open(template_file)?;

        let mut template: Template = serde_json::from_reader(f)?;

        template.sources.sort_by(|d1, d2| d1.start.cmp(&d2.start));

        config.general.template = Some(template);
    }

    if let Some(paths) = args.paths {
        config.storage.paths = paths;
    }

    if let Some(log_path) = args.log {
        if log_path != Path::new("none") {
            config.logging.log_to_file = true;
            config.logging.path = log_path;
        } else {
            config.logging.log_to_file = false;
            config.logging.timestamp = false;
        }
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

    if let Some(level) = args.level {
        if let Ok(filter) = parse_log_level_filter(&level) {
            config.logging.level = filter;
        }
    }

    if args.infinit {
        config.playlist.infinit = args.infinit;
    }

    if let Some(output) = args.output {
        config.out.mode = output;

        if config.out.mode == Null {
            config.out.output_count = 1;
            config.out.output_filter = None;
            config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);
        }
    }

    config.general.skip_validation = args.skip_validation;

    if let Some(volume) = args.volume {
        config.processing.volume = volume;
    }

    Ok(config)
}

/// Format ingest and HLS logging output
pub fn log_line(line: &str, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!("<bright black>[Server]</> {}", line.replace("[info] ", ""))
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            "<bright black>[Server]</> {}",
            line.replace("[warning] ", "")
        )
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!("<bright black>[Server]</> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!("<bright black>[Server]</> {}", line.replace("[fatal] ", ""))
    }
}

/// Compare incoming stream name with expecting name, but ignore question mark.
pub fn valid_stream(msg: &str) -> bool {
    if let Some((unexpected, expected)) = msg.split_once(',') {
        let re = Regex::new(r".*Unexpected stream|expecting|[\s]+|\?$").unwrap();
        let unexpected = re.replace_all(unexpected, "");
        let expected = re.replace_all(expected, "");

        if unexpected == expected {
            return true;
        }
    }

    false
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
    let mut count = 0;
    let re_v = Regex::new(r"\[?0:v(:0)?\]?").unwrap();

    if let Some(mut filter) = filters.clone() {
        for (i, param) in output_params.iter().enumerate() {
            if filter.video_out_link.len() > count && re_v.is_match(param) {
                // replace mapping with link from filter struct
                new_params.push(filter.video_out_link[count].clone());
            } else {
                new_params.push(param.clone());
            }

            // Check if parameter is a output
            if i > 0
                && !param.starts_with('-')
                && !output_params[i - 1].starts_with('-')
                && i < output_params.len() - 1
            {
                count += 1;

                if filter.video_out_link.len() > count
                    && !output_params.contains(&"-map".to_string())
                {
                    new_params.append(&mut vec_strings![
                        "-map",
                        filter.video_out_link[count].clone()
                    ]);

                    for i in 0..config.processing.audio_tracks {
                        new_params.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
                    }
                }
            }
        }

        output_params = new_params;

        cmd.append(&mut filter.cmd());

        // add mapping at the begin, if needed
        if !filter.map().iter().all(|item| output_params.contains(item))
            && filter.output_chain.is_empty()
            && filter.video_out_link.is_empty()
        {
            cmd.append(&mut filter.map())
        } else if &output_params[0] != "-map" && !filter.video_out_link.is_empty() {
            cmd.append(&mut vec_strings!["-map", filter.video_out_link[0].clone()]);

            for i in 0..config.processing.audio_tracks {
                cmd.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
            }
        }
    }

    cmd.append(&mut output_params);

    cmd
}

/// map media struct to json object
pub fn get_media_map(media: Media) -> Value {
    json!({
        "title": media.title,
        "seek": media.seek,
        "out": media.out,
        "duration": media.duration,
        "category": media.category,
        "source": media.source,
    })
}

/// prepare json object for response
pub fn get_data_map(
    config: &PlayoutConfig,
    media: Media,
    playout_stat: &PlayoutStatus,
    server_is_running: bool,
) -> Map<String, Value> {
    let mut data_map = Map::new();
    let current_time = time_in_seconds();
    let shift = *playout_stat.time_shift.lock().unwrap();
    let begin = media.begin.unwrap_or(0.0) - shift;

    data_map.insert("play_mode".to_string(), json!(config.processing.mode));
    data_map.insert("ingest_runs".to_string(), json!(server_is_running));
    data_map.insert("index".to_string(), json!(media.index));
    data_map.insert("start_sec".to_string(), json!(begin));

    if begin > 0.0 {
        let played_time = current_time - begin;
        let remaining_time = media.out - played_time;

        data_map.insert("start_time".to_string(), json!(sec_to_time(begin)));
        data_map.insert("played_sec".to_string(), json!(played_time));
        data_map.insert("remaining_sec".to_string(), json!(remaining_time));
    }

    data_map.insert("current_media".to_string(), get_media_map(media));

    data_map
}
