//! Internal graphics library: native rendering and a CPU compositor backend.
//! No dependency on application playlists, output destinations or the engine crate.
pub mod animation;
pub mod bitmap;
pub mod blend;
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg;
pub mod layout;
pub mod playback;
pub mod playlist;
pub mod scene;
pub mod source;
pub mod style;
pub mod surface;
pub mod text;

fn even(value: u32) -> u32 {
    value & !1
}
