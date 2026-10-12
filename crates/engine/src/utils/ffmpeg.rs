use anyhow::{Context, Result};
use ffmpeg_next::{
    Packet,
    codec::packet::{Mut as PacketMut, Ref as PacketRef},
    ffi, frame,
};

pub(crate) fn reference_video_frame(source: &frame::Video) -> Result<frame::Video> {
    let mut referenced = frame::Video::empty();
    let result = unsafe { ffi::av_frame_ref(referenced.as_mut_ptr(), source.as_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("referencing FFmpeg video frame");
    }

    Ok(referenced)
}

pub(crate) fn reference_audio_frame(source: &frame::Audio) -> Result<frame::Audio> {
    let mut referenced = frame::Audio::empty();
    let result = unsafe { ffi::av_frame_ref(referenced.as_mut_ptr(), source.as_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("referencing FFmpeg audio frame");
    }

    Ok(referenced)
}

pub(crate) fn make_video_frame_writable(frame: &mut frame::Video) -> Result<()> {
    let result = unsafe { ffi::av_frame_make_writable(frame.as_mut_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("making FFmpeg video frame writable");
    }

    Ok(())
}

pub(crate) fn make_audio_frame_writable(frame: &mut frame::Audio) -> Result<()> {
    let result = unsafe { ffi::av_frame_make_writable(frame.as_mut_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("making FFmpeg audio frame writable");
    }

    Ok(())
}

pub(crate) fn reference_packet(source: &Packet) -> Result<Packet> {
    let mut referenced = Packet::empty();
    let result = unsafe { ffi::av_packet_ref(referenced.as_mut_ptr(), source.as_ptr()) };

    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("referencing FFmpeg packet");
    }

    Ok(referenced)
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::format::Pixel;

    use super::*;

    #[test]
    fn referenced_packet_shares_payload_but_has_independent_metadata() {
        let mut source = Packet::copy(&[1, 2, 3, 4]);
        source.set_stream(1);

        let mut referenced = reference_packet(&source).expect("packet reference should succeed");
        assert_eq!(
            source.data().expect("source payload").as_ptr(),
            referenced.data().expect("referenced payload").as_ptr()
        );

        referenced.set_stream(2);
        assert_eq!(source.stream(), 1);
        assert_eq!(referenced.stream(), 2);
    }

    #[test]
    fn writable_referenced_frame_does_not_modify_source_pixels() {
        let mut source = frame::Video::new(Pixel::YUV420P, 4, 4);
        source.data_mut(0)[0] = 17;
        let source_data = source.data(0).as_ptr();

        let mut referenced =
            reference_video_frame(&source).expect("frame reference should succeed");
        assert_eq!(referenced.data(0).as_ptr(), source_data);

        make_video_frame_writable(&mut referenced).expect("frame should become writable");
        referenced.data_mut(0)[0] = 42;

        assert_ne!(referenced.data(0).as_ptr(), source_data);
        assert_eq!(source.data(0)[0], 17);
        assert_eq!(referenced.data(0)[0], 42);
    }
}
