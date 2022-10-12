use ffplayout::input::playlist::gen_source;
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
