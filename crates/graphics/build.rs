use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(ff_graphics_av_alpha_mode)");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");

    if env::var_os("CARGO_FEATURE_FFMPEG").is_none() {
        return;
    }

    // AVFrame's alpha_mode was added after FFmpeg 8.0. Inspect the headers
    // actually used for linking so older FFmpeg installations remain supported.
    let paths = if let Some(root) = env::var_os("FFMPEG_DIR") {
        vec![PathBuf::from(root).join("include")]
    } else {
        pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("libavutil")
            .map(|library| library.include_paths)
            .unwrap_or_default()
    };

    for path in paths {
        let header = path.join("libavutil/frame.h");
        println!("cargo:rerun-if-changed={}", header.display());

        if fs::read_to_string(header)
            .is_ok_and(|text| text.contains("enum AVAlphaMode alpha_mode;"))
        {
            println!("cargo:rustc-cfg=ff_graphics_av_alpha_mode");
            break;
        }
    }
}
