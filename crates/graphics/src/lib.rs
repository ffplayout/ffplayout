//! Internal graphics library: native rendering and a CPU compositor backend.
//! No dependency on application playlists, output destinations or the engine crate.
#[cfg(feature = "ffmpeg")]
pub mod ffmpeg;
pub mod layout;
pub mod rendering;
pub mod resources;
pub mod scene;
pub mod timing;

// Stable entry points used by engine adapters, application code and experiments.
pub use layout::style;
pub use rendering::{bitmap, blend, text};
pub use resources::{preparation, surface};
pub use scene::playlist;
pub use timing::{playback, source, text as animation};

fn even(value: u32) -> u32 {
    value & !1
}
