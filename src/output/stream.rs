use std::process::{self, Command, Stdio};

use simplelog::*;

use crate::filter::v_drawtext;
use crate::utils::{GlobalConfig, Media};
use crate::vec_strings;

/// Streaming Output
///
/// Prepare the ffmpeg command for streaming output
pub fn output(config: &GlobalConfig, log_format: &str) -> process::Child {
    let mut enc_filter = vec![];
    let mut preview = vec![];
    let mut preview_cmd = config.out.preview_cmd.as_ref().unwrap().clone();
    let output_cmd = config.out.output_cmd.as_ref().unwrap().clone();
    let params_len = output_cmd.len();
    let mut output_count = 1;
    let mut output_v_map = "[v_out1]".to_string();
    let mut output_params = output_cmd.clone();
    let mut filter = String::new();

    let mut enc_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format,
        "-re",
        "-i",
        "pipe:0"
    ];

    if config.text.add_text && !config.text.over_pre {
        info!(
            "Using drawtext filter, listening on address: <yellow>{}</>",
            config.text.bind_address
        );

        filter.push_str("[0:v]null,");
        filter.push_str(
            v_drawtext::filter_node(config, &mut Media::new(0, String::new(), false)).as_str(),
        );

        if config.out.preview {
            output_count += 1;
            output_v_map.push_str(format!("[v_out{output_count}]").as_str());

            preview = vec_strings!["-map", "[v_out1]", "-map", "0:a"];
            preview.append(&mut preview_cmd);
            preview.append(&mut vec_strings!["-map", "[v_out2]", "-map", "0:a"]);
        }

        output_params.clear();

        // check for multiple outputs and add mapping to it
        for (i, param) in output_cmd.iter().enumerate() {
            output_params.push(param.clone());

            if i > 0
                && !param.starts_with('-')
                && !output_cmd[i - 1].starts_with('-')
                && i < params_len - 1
            {
                output_count += 1;
                let v_map = format!("[v_out{output_count}]");
                output_v_map.push_str(v_map.as_str());

                let mut map = vec![
                    "-map".to_string(),
                    v_map,
                    "-map".to_string(),
                    "0:a".to_string(),
                ];
                output_params.append(&mut map);
            }
        }

        if output_count > 1 {
            if !filter.is_empty() {
                filter.push(',');
            }

            filter.push_str(format!("split={output_count}{output_v_map}").as_str());

            println!("{}", output_params[0]);

            if preview.is_empty() {
                output_params.insert(0, "-map".to_string());
                output_params.insert(1, "[v_out1]".to_string());
                output_params.insert(2, "-map".to_string());
                output_params.insert(3, "0:a".to_string());
            }
        }

        if !filter.is_empty() {
            enc_filter = vec!["-filter_complex".to_string(), filter];
        }
    } else if config.out.preview {
        preview = preview_cmd;
    }

    enc_cmd.append(&mut enc_filter);
    enc_cmd.append(&mut preview);
    enc_cmd.append(&mut output_params);

    debug!(
        "Encoder CMD: <bright-blue>\"ffmpeg {}\"</>",
        enc_cmd.join(" ")
    );

    let enc_proc = match Command::new("ffmpeg")
        .args(enc_cmd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            error!("couldn't spawn encoder process: {e}");
            panic!("couldn't spawn encoder process: {e}")
        }
        Ok(proc) => proc,
    };

    enc_proc
}
