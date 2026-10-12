fn main() {
    println!("cargo:rustc-check-cfg=cfg(ffplayout_srt_linked)");

    // SRT is optional in FFmpeg builds. Only reference its C API when a
    // linkable library is available for this target.
    if pkg_config::Config::new().probe("srt").is_ok() {
        println!("cargo:rustc-cfg=ffplayout_srt_linked");
    }
}
