use std::{
    fs,
    sync::{Arc, Mutex},
};

use ffplayout::{input::playlist::gen_source, utils::prepare_output_cmd};
use ffplayout_lib::{
    filter::v_drawtext,
    utils::{Media, OutputMode::*, PlayoutConfig},
    vec_strings,
};

#[test]
fn video_audio_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = true;
    let logo_path = fs::canonicalize("./assets/logo.png").unwrap();
    config.processing.logo = logo_path.to_string_lossy().to_string();

    println!("{:?}", config.processing.logo);
    println!("--is file {:?}", logo_path.is_file());

    let media_obj = Media::new(0, "./assets/with_audio.mp4", true);
    println!("{media_obj:?}");
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));

    println!("{:?}", media.filter);
    println!("category {:?}", media.category);

    let test_filter_cmd = Some(
        vec_strings![
            "-filter_complex",
            format!("[0:v:0]scale=1024:576,null[v];movie={}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa=0.7[l];[v][l]overlay=W-w-12:12:shortest=1[vout0];[0:a:0]anull[aout0]", config.processing.logo),
            "-map",
            "[vout0]",
            "-map",
            "[aout0]"
        ],
    );

    assert_eq!(
        media.cmd,
        Some(vec_strings!["-i", "./assets/with_audio.mp4"])
    );
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn dual_audio_aevalsrc_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.audio_tracks = 2;
    config.processing.add_logo = false;

    let media_obj = Media::new(0, "./assets/with_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));

    let test_filter_cmd = Some(
        vec_strings![
            "-filter_complex",
            "[0:v:0]scale=1024:576[vout0];[0:a:0]anull[aout0];aevalsrc=0:channel_layout=stereo:duration=30:sample_rate=48000,anull[aout1]",
            "-map",
            "[vout0]",
            "-map",
            "[aout0]",
            "-map",
            "[aout1]"
        ],
    );

    assert_eq!(
        media.cmd,
        Some(vec_strings!["-i", "./assets/with_audio.mp4"])
    );
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn dual_audio_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.audio_tracks = 2;
    config.processing.add_logo = false;

    let media_obj = Media::new(0, "./assets/dual_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));

    let test_filter_cmd = Some(vec_strings![
        "-filter_complex",
        "[0:v:0]scale=1024:576[vout0];[0:a:0]anull[aout0];[0:a:1]anull[aout1]",
        "-map",
        "[vout0]",
        "-map",
        "[aout0]",
        "-map",
        "[aout1]"
    ]);

    assert_eq!(
        media.cmd,
        Some(vec_strings!["-i", "./assets/dual_audio.mp4"])
    );
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn video_audio_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream"
    ]);

    let mut enc_cmd = vec![];
    let enc_filter = vec![];
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_dual_audio_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051"
    ]);

    let mut enc_cmd = vec![];
    let enc_filter = vec![];
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_dual_audio_filter_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.text.fontfile = String::new();
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051"
    ]);

    let mut enc_cmd = vec![];
    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    let socket = config
        .text
        .zmq_stream_socket
        .clone()
        .unwrap()
        .replace(':', "\\:");
    let mut filter = "[0:v]null,".to_string();

    filter.push_str(v_drawtext::filter_node(&config, None, &Arc::new(Mutex::new(vec![]))).as_str());

    let enc_filter = vec!["-filter_complex".to_string(), filter];

    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-filter_complex",
        format!("[0:v]null,zmq=b=tcp\\\\://'{socket}',drawtext=text=''[v_out1]"),
        "-map",
        "[v_out1]",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_audio_multi_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost:1936/live/stream"
    ]);

    let mut enc_cmd = vec![];
    let enc_filter = vec![];
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost:1936/live/stream"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_dual_audio_multi_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.out.output_cmd = Some(vec_strings![
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051",
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40052"
    ]);

    let mut enc_cmd = vec![];
    let enc_filter = vec![];
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051",
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40052"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_dual_audio_multi_filter_stream() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = Stream;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.text.fontfile = String::new();
    config.out.output_cmd = Some(vec_strings![
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051",
        "-map",
        "0:v",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40052"
    ]);

    let mut enc_cmd = vec![];
    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];

    let socket = config
        .text
        .zmq_stream_socket
        .clone()
        .unwrap()
        .replace(':', "\\:");
    let mut filter = "[0:v]null,".to_string();

    filter.push_str(v_drawtext::filter_node(&config, None, &Arc::new(Mutex::new(vec![]))).as_str());

    let enc_filter = vec!["-filter_complex".to_string(), filter];

    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0",
        "-filter_complex",
        format!("[0:v]null,zmq=b=tcp\\\\://'{socket}',drawtext=text=''[v_out1]"),
        "-map",
        "[v_out1]",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40051",
        "-map",
        "[v_out1]",
        "-map",
        "0:a:0",
        "-map",
        "0:a:1",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+global_header",
        "-f",
        "mpegts",
        "srt://127.0.0.1:40052"
    ];

    // println!("{enc_cmd:?}");

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_audio_hls() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = HLS;
    config.processing.add_logo = false;
    config.text.add_text = false;
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+cgop",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream-%d.ts",
        "/usr/share/ffplayout/public/live/stream.m3u8"
    ]);

    let media_obj = Media::new(0, "./assets/with_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));
    let enc_filter = media.filter.unwrap();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/with_audio.mp4"
    ];

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/with_audio.mp4",
        "-filter_complex",
        "[0:v:0]scale=1024:576,realtime=speed=1[vout0];[0:a:0]anull[aout0]",
        "-map",
        "[vout0]",
        "-map",
        "[aout0]",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+cgop",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream-%d.ts",
        "/usr/share/ffplayout/public/live/stream.m3u8"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn video_multi_audio_hls() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = HLS;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.text.add_text = false;
    config.out.output_cmd = Some(vec_strings![
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+cgop",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream-%d.ts",
        "/usr/share/ffplayout/public/live/stream.m3u8"
    ]);

    let media_obj = Media::new(0, "./assets/dual_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));
    let enc_filter = media.filter.unwrap();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/dual_audio.mp4"
    ];

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/dual_audio.mp4",
        "-filter_complex",
        "[0:v:0]scale=1024:576,realtime=speed=1[vout0];[0:a:0]anull[aout0];[0:a:1]anull[aout1]",
        "-map",
        "[vout0]",
        "-map",
        "[aout0]",
        "-map",
        "[aout1]",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-ar",
        "44100",
        "-b:a",
        "128k",
        "-flags",
        "+cgop",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream-%d.ts",
        "/usr/share/ffplayout/public/live/stream.m3u8"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn multi_video_audio_hls() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = HLS;
    config.processing.add_logo = false;
    config.text.add_text = false;
    config.out.output_cmd = Some(vec_strings![
        "-filter_complex",
        "[0:v]split=2[v1_out][v2];[v2]scale=w=512:h=288[v2_out];[0:a]asplit=2[a1][a2]",
        "-map",
        "[v1_out]",
        "-map",
        "[a1]",
        "-c:v",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a",
        "aac",
        "-map",
        "[v2_out]",
        "-map",
        "[a2]",
        "-c:v:1",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a:1",
        "aac",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream_%v-%d.ts",
        "-master_pl_name",
        "master.m3u8",
        "-var_stream_map",
        "v:0,a:0,name:720p v:1,a:1,name:288p",
        "/usr/share/ffplayout/public/live/stream_%v.m3u8"
    ]);

    let media_obj = Media::new(0, "./assets/with_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));
    let enc_filter = media.filter.unwrap();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/with_audio.mp4"
    ];

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/with_audio.mp4",
        "-filter_complex",
        "[0:v:0]scale=1024:576,realtime=speed=1[vout0];[0:a:0]anull[aout0];[vout0]split=2[v1_out][v2];[v2]scale=w=512:h=288[v2_out];[aout0]asplit=2[a1][a2]",
        "-map",
        "[v1_out]",
        "-map",
        "[a1]",
        "-c:v",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a",
        "aac",
        "-map",
        "[v2_out]",
        "-map",
        "[a2]",
        "-c:v:1",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a:1",
        "aac",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream_%v-%d.ts",
        "-master_pl_name",
        "master.m3u8",
        "-var_stream_map",
        "v:0,a:0,name:720p v:1,a:1,name:288p",
        "/usr/share/ffplayout/public/live/stream_%v.m3u8"
    ];

    assert_eq!(enc_cmd, test_cmd);
}

#[test]
fn multi_video_multi_audio_hls() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = HLS;
    config.processing.add_logo = false;
    config.processing.audio_tracks = 2;
    config.text.add_text = false;
    config.out.output_cmd = Some(vec_strings![
        "-filter_complex",
        "[0:v]split=2[v1_out][v2];[v2]scale=w=512:h=288[v2_out];[0:a:0]asplit=2[a_0_1][a_0_2];[0:a:1]asplit=2[a_1_1][a_1_2]",
        "-map",
        "[v1_out]",
        "-map",
        "[a_0_1]",
        "-map",
        "[a_1_1]",
        "-c:v",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a",
        "aac",
        "-map",
        "[v2_out]",
        "-map",
        "[a_0_2]",
        "-map",
        "[a_1_2]",
        "-c:v:1",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a:1",
        "aac",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream_%v-%d.ts",
        "-master_pl_name",
        "master.m3u8",
        "-var_stream_map",
        "v:0,a:0,a:1,name:720p v:1,a:2,a:3,name:288p",
        "/usr/share/ffplayout/public/live/stream_%v.m3u8"
    ]);

    let media_obj = Media::new(0, "./assets/dual_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));
    let enc_filter = media.filter.unwrap();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/dual_audio.mp4"
    ];

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, &config);

    let test_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "./assets/dual_audio.mp4",
        "-filter_complex",
        "[0:v:0]scale=1024:576,realtime=speed=1[vout0];[0:a:0]anull[aout0];[0:a:1]anull[aout1];[vout0]split=2[v1_out][v2];[v2]scale=w=512:h=288[v2_out];[aout0]asplit=2[a_0_1][a_0_2];[aout1]asplit=2[a_1_1][a_1_2]",
        "-map",
        "[v1_out]",
        "-map",
        "[a_0_1]",
        "-map",
        "[a_1_1]",
        "-c:v",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a",
        "aac",
        "-map",
        "[v2_out]",
        "-map",
        "[a_0_2]",
        "-map",
        "[a_1_2]",
        "-c:v:1",
        "libx264",
        "-flags",
        "+cgop",
        "-c:a:1",
        "aac",
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "600",
        "-hls_flags",
        "append_list+delete_segments+omit_endlist",
        "-hls_segment_filename",
        "/usr/share/ffplayout/public/live/stream_%v-%d.ts",
        "-master_pl_name",
        "master.m3u8",
        "-var_stream_map",
        "v:0,a:0,a:1,name:720p v:1,a:2,a:3,name:288p",
        "/usr/share/ffplayout/public/live/stream_%v.m3u8"
    ];

    assert_eq!(enc_cmd, test_cmd);
}
