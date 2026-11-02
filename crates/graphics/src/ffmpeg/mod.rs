//! FFmpeg CPU backend. Frame handles stay outside scene/style and native renderer APIs.
use anyhow::{Context, Result};
use ffmpeg_next::{ffi, frame};

pub mod logo;
pub mod overlay;
pub mod png;
pub mod scene;
pub mod source;
pub mod surface;
pub mod text;

pub fn reference_video_frame(source: &frame::Video) -> Result<frame::Video> {
    let mut referenced = frame::Video::empty();
    let result = unsafe { ffi::av_frame_ref(referenced.as_mut_ptr(), source.as_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("referencing FFmpeg video frame");
    }

    Ok(referenced)
}
