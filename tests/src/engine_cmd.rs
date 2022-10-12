use ffplayout::{input::playlist::gen_source, utils::prepare_output_cmd};
use ffplayout_lib::{
    utils::{Media, PlayoutConfig},
    vec_strings,
};
use std::sync::{Arc, Mutex};

#[test]
fn video_audio_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = "stream".to_string();
    config.processing.logo = "../assets/logo.png".to_string();

    let media_obj = Media::new(0, "assets/with_audio.mp4", true);
    let media = gen_source(&config, media_obj, &Arc::new(Mutex::new(vec![])));

    let test_filter_cmd = Some(
        vec_strings![
            "-filter_complex",
            "[0:v:0]scale=1024:576,null[v];movie=../assets/logo.png:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa=0.7[l];[v][l]overlay=W-w-12:12:shortest=1[vout0];[0:a:0]anull[aout0]",
            "-map",
            "[vout0]",
            "-map",
            "[aout0]"
        ],
    );

    assert_eq!(media.cmd, Some(vec_strings!["-i", "assets/with_audio.mp4"]));
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn dual_audio_aevalsrc_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = "stream".to_string();
    config.processing.audio_tracks = 2;
    config.processing.add_logo = false;

    let media_obj = Media::new(0, "assets/with_audio.mp4", true);
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

    assert_eq!(media.cmd, Some(vec_strings!["-i", "assets/with_audio.mp4"]));
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn dual_audio_input() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = "stream".to_string();
    config.processing.audio_tracks = 2;
    config.processing.add_logo = false;

    let media_obj = Media::new(0, "assets/dual_audio.mp4", true);
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

    assert_eq!(media.cmd, Some(vec_strings!["-i", "assets/dual_audio.mp4"]));
    assert_eq!(media.filter, test_filter_cmd);
}

#[test]
fn test_prepare_output_cmd() {
    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-re",
        "-i",
        "pipe:0"
    ];
    let filter = vec_strings![
        "-filter_complex",
        "[0:v]null,zmq=b=tcp\\\\://'127.0.0.1\\:5555',drawtext=text=''"
    ];
    let params = vec_strings![
        "-c:v",
        "libx264",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost/live/stream",
        "-s",
        "512x288",
        "-c:v",
        "libx264",
        "-flags",
        "+global_header",
        "-f",
        "flv",
        "rtmp://localhost:1937/live/stream"
    ];

    let mut t1_params = enc_prefix.clone();
    t1_params.append(&mut params.clone());
    let cmd_two_outs =
        prepare_output_cmd(enc_prefix.clone(), vec_strings![], params.clone(), "stream");

    assert_eq!(cmd_two_outs, t1_params);

    let mut test_cmd = enc_prefix.clone();
    let mut test_params = params.clone();
    let mut t2_filter = filter.clone();
    t2_filter[1].push_str(",split=2[v_out1][v_out2]");
    test_cmd.append(&mut t2_filter);

    test_params.insert(0, "-map".to_string());
    test_params.insert(1, "[v_out1]".to_string());
    test_params.insert(2, "-map".to_string());
    test_params.insert(3, "0:a".to_string());

    test_params.insert(11, "-map".to_string());
    test_params.insert(12, "[v_out2]".to_string());
    test_params.insert(13, "-map".to_string());
    test_params.insert(14, "0:a".to_string());

    test_cmd.append(&mut test_params);
    let cmd_two_outs_with_filter = prepare_output_cmd(enc_prefix, filter, params, "stream");

    assert_eq!(cmd_two_outs_with_filter, test_cmd);
}

#[test]
fn video_audio_output() {
    let mut config = PlayoutConfig::new(Some("../assets/ffplayout.yml".to_string()));
    config.out.mode = "stream".to_string();
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

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, enc_cmd, &config.out.mode);

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
