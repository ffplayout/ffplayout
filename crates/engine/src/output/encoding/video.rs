use std::{ffi::CString, ptr};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{
    self as ffmpeg, Packet, Rational, codec, frame, software::scaling, util::format::pixel::Pixel,
};

use crate::{HlsVariant, OutputConfig, utils::config::video_codec_uses_bitrate};

pub(in crate::output) struct VideoOutputStream {
    pub(in crate::output) stream_index: usize,
    pub(in crate::output) encoder: codec::encoder::video::Encoder,
    pub(in crate::output) scaler: Option<scaling::Context>,
    scaled_frame: Option<frame::Video>,
    vaapi_upload: Option<VaapiUpload>,
    cpu_format: Pixel,
}

// SAFETY: all fields are exclusively owned. In addition to FFmpeg's Send
// codec/frame wrappers, the SwsContext is moved once to its encoder worker and
// is only used and freed there; it has no thread-affine application callbacks.
unsafe impl Send for VideoOutputStream {}

impl VideoOutputStream {
    pub(in crate::output) fn submit(&mut self, frame: &frame::Video) -> Result<()> {
        let needs_scale = frame.width() != self.encoder.width()
            || frame.height() != self.encoder.height()
            || frame.format() != self.cpu_format;
        if needs_scale
            && !self.scaler.as_ref().is_some_and(|scaler| {
                let input = scaler.input();
                input.width == frame.width()
                    && input.height == frame.height()
                    && input.format == frame.format()
            })
        {
            self.scaler = Some(scaling::Context::get(
                frame.format(),
                frame.width(),
                frame.height(),
                self.cpu_format,
                self.encoder.width(),
                self.encoder.height(),
                scaling::flag::Flags::BILINEAR,
            )?);
            self.scaled_frame = None;
        }
        let input = if needs_scale && let Some(scaler) = &mut self.scaler {
            let scaled = self.scaled_frame.get_or_insert_with(frame::Video::empty);
            if scaled.width() != 0 {
                crate::utils::ffmpeg::make_video_frame_writable(scaled)?;
            }
            scaled.set_pts(frame.pts());
            scaler.run(frame, scaled)?;
            scaled
        } else {
            frame
        };

        if let Some(upload) = &mut self.vaapi_upload {
            self.encoder.send_frame(upload.upload(input)?)?;
        } else {
            self.encoder.send_frame(input)?;
        }

        Ok(())
    }

    pub(in crate::output) fn drain(
        &mut self,
        mut emit: impl FnMut(Packet, usize, Rational) -> Result<()>,
    ) -> Result<()> {
        let mut packet = Packet::empty();

        while self.encoder.receive_packet(&mut packet).is_ok() {
            if packet.duration() == 0 {
                packet.set_duration(1);
            }
            emit(packet, self.stream_index, self.encoder.time_base())?;
            packet = Packet::empty();
        }

        Ok(())
    }
}

/// Owns the VAAPI frame pool used to upload the CPU-composited NV12 frame
/// immediately before passing it to a VAAPI encoder.
struct VaapiUpload {
    frames_ctx: *mut ffmpeg::ffi::AVBufferRef,
    frame: frame::Video,
}

unsafe impl Send for VaapiUpload {}

impl VaapiUpload {
    fn new(width: u32, height: u32) -> Result<Self> {
        let device = CString::new("/dev/dri/renderD128").expect("static VAAPI device path");
        let mut device_ctx = ptr::null_mut();
        let result = unsafe {
            ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut device_ctx,
                ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                device.as_ptr(),
                ptr::null_mut(),
                0,
            )
        };

        if result < 0 {
            return Err(ffmpeg::Error::from(result))
                .context("failed to create VAAPI device /dev/dri/renderD128");
        }

        if device_ctx.is_null() {
            return Err(anyhow!("VAAPI device creation returned no device context"));
        }

        let frames_ctx = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device_ctx) };
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut device_ctx) };

        if frames_ctx.is_null() {
            return Err(anyhow!("failed to allocate VAAPI frame context"));
        }

        let frames = unsafe { (*frames_ctx).data.cast::<ffmpeg::ffi::AVHWFramesContext>() };

        if frames.is_null() {
            unsafe { ffmpeg::ffi::av_buffer_unref(&mut (frames_ctx as *mut _)) };

            return Err(anyhow!("VAAPI frame context has no frame pool data"));
        }
        unsafe {
            (*frames).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
            (*frames).sw_format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NV12;
            (*frames).width = width as i32;
            (*frames).height = height as i32;
        }

        let result = unsafe { ffmpeg::ffi::av_hwframe_ctx_init(frames_ctx) };

        if result < 0 {
            unsafe { ffmpeg::ffi::av_buffer_unref(&mut (frames_ctx as *mut _)) };

            return Err(ffmpeg::Error::from(result))
                .context("failed to initialize VAAPI frame context");
        }

        Ok(Self {
            frames_ctx,
            frame: frame::Video::empty(),
        })
    }

    fn attach_to_encoder(&self, video_ctx: &mut codec::encoder::video::Video) -> Result<()> {
        let frames_ctx = unsafe { ffmpeg::ffi::av_buffer_ref(self.frames_ctx) };

        if frames_ctx.is_null() {
            return Err(anyhow!("failed to retain VAAPI frame context for encoder"));
        }
        unsafe { (*video_ctx.as_mut_ptr()).hw_frames_ctx = frames_ctx };

        Ok(())
    }

    fn upload(&mut self, source: &frame::Video) -> Result<&frame::Video> {
        unsafe { ffmpeg::ffi::av_frame_unref(self.frame.as_mut_ptr()) };
        let result = unsafe {
            ffmpeg::ffi::av_hwframe_get_buffer(self.frames_ctx, self.frame.as_mut_ptr(), 0)
        };

        if result < 0 {
            return Err(ffmpeg::Error::from(result)).context("failed to allocate VAAPI frame");
        }

        let result = unsafe {
            ffmpeg::ffi::av_hwframe_transfer_data(self.frame.as_mut_ptr(), source.as_ptr(), 0)
        };

        if result < 0 {
            return Err(ffmpeg::Error::from(result)).context("failed to upload frame to VAAPI");
        }

        let result =
            unsafe { ffmpeg::ffi::av_frame_copy_props(self.frame.as_mut_ptr(), source.as_ptr()) };

        if result < 0 {
            return Err(ffmpeg::Error::from(result))
                .context("failed to copy VAAPI frame properties");
        }

        Ok(&self.frame)
    }
}

impl Drop for VaapiUpload {
    fn drop(&mut self) {
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.frames_ctx) };
    }
}

pub(in crate::output) fn open_video_encoder(
    stream_index: usize,
    cfg: &OutputConfig,
    global_header: bool,
    variant: Option<&HlsVariant>,
    input_size: (u32, u32),
    gop_size: Option<u32>,
    closed_gop: bool,
) -> Result<VideoOutputStream> {
    let video_codec = if cfg.video_codec.trim().is_empty() {
        codec::encoder::find(codec::Id::H264)
    } else {
        codec::encoder::find_by_name(cfg.video_codec.trim())
    }
    .with_context(|| format!("video encoder {:?} not found", cfg.video_codec))?;
    let mut video_ctx = codec::context::Context::new_with_codec(video_codec)
        .encoder()
        .video()?;
    let encoder_backend = VideoEncoderBackend::from_name(video_codec.name());
    let encoder_format = encoder_backend.encoder_format();
    let cpu_input_format = encoder_backend.cpu_input_format();
    let width = variant.map_or(cfg.width, |variant| variant.width);
    let height = variant.map_or(cfg.height, |variant| variant.height);
    let (input_width, input_height) = input_size;
    video_ctx.set_width(width);
    video_ctx.set_height(height);
    video_ctx.set_format(encoder_format);
    video_ctx.set_time_base(cfg.video_time_base);
    video_ctx.set_frame_rate(Some(Rational(cfg.fps as i32, 1)));
    let maxrate = variant.map_or(cfg.video_maxrate(), |variant| variant.video_bitrate);

    if encoder_backend.uses_target_bitrate(cfg, video_codec) {
        video_ctx.set_bit_rate(maxrate as usize);
    }

    if let Some(gop_size) = gop_size {
        video_ctx.set_gop(gop_size);
    }

    let mut video_flags = codec::flag::Flags::empty();

    if global_header {
        video_flags |= codec::flag::Flags::GLOBAL_HEADER;
    }

    if closed_gop {
        video_flags |= codec::flag::Flags::CLOSED_GOP;
    }

    if encoder_backend == VideoEncoderBackend::Qsv && qsv_uses_icq(cfg) {
        // QSV selects ICQ from AVCodecContext::global_quality. Passing this
        // through the encoder option dictionary does not reliably update the
        // context before rate control is selected.
        let global_quality = qsv_global_quality(cfg);
        log::debug!(channel = cfg.channel_id.unwrap_or_default(); "QSV encoder rate control: ICQ, global quality: {global_quality}");
        video_ctx.set_global_quality(global_quality);
    }

    if !video_flags.is_empty() {
        video_ctx.set_flags(video_flags);
    }

    let vaapi_upload = (encoder_backend == VideoEncoderBackend::Vaapi)
        .then(|| VaapiUpload::new(width, height))
        .transpose()?;

    if let Some(vaapi_upload) = &vaapi_upload {
        vaapi_upload.attach_to_encoder(&mut video_ctx)?;
    }

    let mut options = ffmpeg::Dictionary::new();
    encoder_backend.configure_options(&mut options, cfg, maxrate);

    if closed_gop && encoder_backend == VideoEncoderBackend::X264 {
        options.set("x264-params", "open-gop=0:repeat-headers=1");
    }

    let mut video_encoder = video_ctx.open_as_with(video_codec, options)?;
    // Quality-based encoders clear AVCodecContext::bit_rate while opening.
    // Restore the configured maximum as metadata so the HLS muxer can
    // calculate BANDWIDTH for master.m3u8. QSV and VAAPI observe a bitrate
    // change when their quality-only mode begins, which would invalidate it.
    if video_codec_uses_bitrate(video_codec.name())
        && restore_video_bitrate_metadata(encoder_backend, cfg)
    {
        video_encoder.set_bit_rate(maxrate as usize);
    }

    let scaler =
        if width == input_width && height == input_height && cpu_input_format == Pixel::YUV420P {
            None
        } else {
            Some(scaling::Context::get(
                Pixel::YUV420P,
                input_width,
                input_height,
                cpu_input_format,
                width,
                height,
                scaling::flag::Flags::BILINEAR,
            )?)
        };

    Ok(VideoOutputStream {
        stream_index,
        encoder: video_encoder,
        scaler,
        scaled_frame: None,
        vaapi_upload,
        cpu_format: cpu_input_format,
    })
}
/// Encoders in this pipeline receive CPU-backed frames. QSV accepts NV12 and
/// uploads it internally; VAAPI receives an NV12 frame uploaded to a VAAPI
/// frame pool immediately before encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoEncoderBackend {
    Software,
    X264,
    X265,
    Nvenc,
    Qsv,
    Vaapi,
    VpxVp9,
    SvtAv1,
}

impl VideoEncoderBackend {
    pub(in crate::output) fn from_name(name: &str) -> Self {
        if name.contains("x264") {
            Self::X264
        } else if name.contains("x265") {
            Self::X265
        } else if matches!(name, "h264_nvenc" | "hevc_nvenc" | "av1_nvenc") {
            Self::Nvenc
        } else if matches!(name, "h264_qsv" | "hevc_qsv" | "av1_qsv") {
            Self::Qsv
        } else if matches!(name, "h264_vaapi" | "hevc_vaapi" | "av1_vaapi") {
            Self::Vaapi
        } else if name == "libvpx-vp9" {
            Self::VpxVp9
        } else if name == "libsvtav1" {
            Self::SvtAv1
        } else {
            Self::Software
        }
    }

    pub(in crate::output) const fn encoder_format(self) -> Pixel {
        match self {
            Self::Vaapi => Pixel::VAAPI,
            _ => self.cpu_input_format(),
        }
    }

    pub(in crate::output) const fn cpu_input_format(self) -> Pixel {
        match self {
            Self::Qsv => Pixel::NV12,
            Self::Vaapi => Pixel::NV12,
            Self::Software
            | Self::X264
            | Self::X265
            | Self::Nvenc
            | Self::VpxVp9
            | Self::SvtAv1 => Pixel::YUV420P,
        }
    }

    pub(in crate::output) fn uses_target_bitrate(
        self,
        cfg: &OutputConfig,
        codec: codec::codec::Codec,
    ) -> bool {
        cfg.video_option("rate_control") == Some("cbr")
            || self == Self::VpxVp9
            || (self == Self::Vaapi && cfg.video_option("rate_control") == Some("vbr"))
            // Encoders that fall through to the generic Software backend and
            // expose none of the rate-control AVOptions this pipeline sets
            // (maxrate/bufsize/crf/qp) only understand a plain target
            // bitrate via AVCodecContext::bit_rate. Detecting this
            // capability at runtime - instead of matching specific codec
            // names such as "h264_v4l2m2m" - covers every current and
            // future encoder with the same limitation (other stateless HW
            // wrappers included) without growing a per-codec list.
            || (self == Self::Software && video_encoder_is_bitrate_only(codec))
    }

    pub(in crate::output) fn configure_options(
        self,
        options: &mut ffmpeg::Dictionary<'_>,
        cfg: &OutputConfig,
        maxrate: u64,
    ) {
        match self {
            Self::X264 | Self::X265 => {
                options.set("preset", cfg.video_option("preset").unwrap_or("faster"));
                let tune = cfg.video_option("tune").unwrap_or("zerolatency");
                if tune != "none" {
                    options.set("tune", tune);
                }
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());

                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("minrate", &maxrate.to_string()),
                    _ => options.set("crf", cfg.video_option("quality").unwrap_or("23")),
                }
            }
            Self::Nvenc => {
                options.set("preset", "p4");
                options.set("tune", "ll");
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());

                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("rc", "cbr"),
                    _ => {
                        options.set("rc", "vbr");
                        options.set("cq", cfg.video_option("quality").unwrap_or("23"));
                    }
                }
            }
            Self::Qsv => {
                options.set("preset", cfg.video_option("preset").unwrap_or("faster"));
                options.set("async_depth", "4");
                options.set("low_delay_brc", "1");

                if !qsv_uses_icq(cfg) {
                    options.set("maxrate", &maxrate.to_string());
                    options.set("bufsize", &maxrate.saturating_mul(2).to_string());

                    if cfg.video_option("rate_control") == Some("cbr") {
                        options.set("rdo", "0");
                    }
                }
            }
            Self::Vaapi => {
                let rate_control = cfg.video_option("rate_control").unwrap_or("vbr");
                options.set("async_depth", "4");
                options.set(
                    "rc_mode",
                    match rate_control {
                        "cqp" => "CQP",
                        "cbr" => "CBR",
                        _ => "VBR",
                    },
                );

                if rate_control == "cqp" {
                    options.set("qp", cfg.video_option("quality").unwrap_or("23"));
                } else {
                    options.set("maxrate", &maxrate.to_string());
                    options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                }
            }
            Self::VpxVp9 => {
                options.set("deadline", cfg.video_option("deadline").unwrap_or("good"));
                options.set("cpu-used", cfg.video_option("cpu-used").unwrap_or("4"));
                options.set("row-mt", cfg.video_option("row-mt").unwrap_or("auto"));
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());

                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("minrate", &maxrate.to_string()),
                    _ => options.set("crf", cfg.video_option("quality").unwrap_or("31")),
                }
            }
            Self::SvtAv1 => {
                options.set("preset", cfg.video_option("preset").unwrap_or("8"));
                options.set("crf", cfg.video_option("quality").unwrap_or("30"));
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());
            }
            Self::Software => {}
        }
    }
}

/// Checks whether an encoder's private option class (`AVCodec::priv_class`)
/// declares the given option name, without needing to open/instantiate the
/// encoder. This mirrors what `ffmpeg -h encoder=<name>` shows under
/// "AVOptions" and is the generic, ffmpeg-native way to introspect what an
/// encoder actually supports at runtime.
fn video_encoder_has_option(codec: codec::codec::Codec, name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };

    unsafe {
        let priv_class = (*codec.as_ptr()).priv_class;

        if priv_class.is_null() {
            return false;
        }

        !ffmpeg::ffi::av_opt_find(
            ptr::from_ref(&priv_class).cast_mut().cast(),
            name.as_ptr(),
            ptr::null(),
            0,
            ffmpeg::ffi::AV_OPT_SEARCH_FAKE_OBJ,
        )
        .is_null()
    }
}

/// Some encoders (most notably stateless HW wrapper encoders like the V4L2
/// M2M drivers used on Raspberry Pi / embedded SoCs) expose none of the
/// rate-control AVOptions this pipeline normally configures (maxrate,
/// bufsize, crf, qp). They only honor a plain target bitrate set on
/// `AVCodecContext::bit_rate`, otherwise the underlying driver falls back
/// to its own default (observed around 300-400kbps on a Raspberry Pi 4).
/// Detecting this by capability instead of by codec name means any encoder
/// with the same limitation - present today or added to ffmpeg later - is
/// handled automatically, without maintaining a per-codec list here.
fn video_encoder_is_bitrate_only(codec: codec::codec::Codec) -> bool {
    ["maxrate", "bufsize", "crf", "qp"]
        .iter()
        .all(|option| !video_encoder_has_option(codec, option))
}

fn qsv_uses_icq(cfg: &OutputConfig) -> bool {
    cfg.video_option("rate_control") == Some("icq")
}

fn qsv_global_quality(cfg: &OutputConfig) -> i32 {
    cfg.video_option("global_quality")
        .and_then(|value| value.parse().ok())
        .filter(|value| (1..=51).contains(value))
        .unwrap_or(23)
}

fn restore_video_bitrate_metadata(backend: VideoEncoderBackend, cfg: &OutputConfig) -> bool {
    !(backend == VideoEncoderBackend::Qsv && qsv_uses_icq(cfg)
        || backend == VideoEncoderBackend::Vaapi && cfg.video_option("rate_control") == Some("cqp"))
}

/// Use a keyframe interval that fits exactly into the requested HLS segment.
/// Keeping it at two seconds or less balances segment precision and bitrate.
pub(in crate::output) fn hls_gop_size(fps: u32, segment_seconds: u32) -> u32 {
    let segment_seconds = segment_seconds.max(1);
    let gop_seconds = (1..=segment_seconds.min(2))
        .rev()
        .find(|seconds| segment_seconds.is_multiple_of(*seconds))
        .unwrap_or(1);

    fps.saturating_mul(gop_seconds)
}

pub(in crate::output) fn stream_gop_size(fps: u32) -> u32 {
    fps.saturating_mul(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_accepts_target_size_composites_and_original_frames_during_logo_fades() {
        ffmpeg::init().unwrap();
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config
            .video_options
            .insert("tune".into(), "zerolatency".into());
        config.video_options.insert("crf".into(), "0".into());
        let mut stream =
            open_video_encoder(0, &config, true, None, (320, 180), Some(25), true).unwrap();
        let parameters = codec::Parameters::from(&stream.encoder);
        let mut packets = Vec::new();
        for index in 0..8 {
            let (width, height) = if index % 2 == 0 {
                (320, 180)
            } else {
                (160, 90)
            };
            let mut frame = frame::Video::new(Pixel::YUV420P, width, height);
            frame.data_mut(0).fill(32 + index * 16);
            frame.data_mut(1).fill(128);
            frame.data_mut(2).fill(128);
            frame.set_pts(Some(i64::from(index)));
            stream.submit(&frame).unwrap();
            stream
                .drain(|packet, _, _| {
                    packets.push(packet);
                    Ok(())
                })
                .unwrap();
        }
        stream.encoder.send_eof().unwrap();
        stream
            .drain(|packet, _, _| {
                packets.push(packet);
                Ok(())
            })
            .unwrap();
        let mut decoder = codec::context::Context::from_parameters(parameters)
            .unwrap()
            .decoder()
            .video()
            .unwrap();
        let mut pixels = Vec::new();
        for packet in packets {
            decoder.send_packet(&packet).unwrap();
            let mut frame = frame::Video::empty();
            while decoder.receive_frame(&mut frame).is_ok() {
                assert_eq!((frame.width(), frame.height()), (160, 90));
                pixels.push(frame.data(0)[0]);
            }
        }
        decoder.send_eof().unwrap();
        let mut frame = frame::Video::empty();
        while decoder.receive_frame(&mut frame).is_ok() {
            pixels.push(frame.data(0)[0]);
        }
        assert_eq!(
            pixels,
            (0..8).map(|index| 32 + index * 16).collect::<Vec<u8>>()
        );
    }

    #[test]
    fn selects_nvenc_for_cpu_backed_hardware_encoding() {
        assert_eq!(
            VideoEncoderBackend::from_name("h264_nvenc"),
            VideoEncoderBackend::Nvenc
        );
        assert_eq!(
            VideoEncoderBackend::from_name("hevc_nvenc"),
            VideoEncoderBackend::Nvenc
        );
        assert_eq!(
            VideoEncoderBackend::from_name("libx264"),
            VideoEncoderBackend::X264
        );
        assert_eq!(
            VideoEncoderBackend::from_name("libx265"),
            VideoEncoderBackend::X265
        );
        assert_eq!(
            VideoEncoderBackend::from_name("h264_qsv"),
            VideoEncoderBackend::Qsv
        );
        assert_eq!(VideoEncoderBackend::Qsv.cpu_input_format(), Pixel::NV12);
        assert_eq!(
            VideoEncoderBackend::from_name("h264_vaapi"),
            VideoEncoderBackend::Vaapi
        );
        assert_eq!(VideoEncoderBackend::Vaapi.cpu_input_format(), Pixel::NV12);
        assert_eq!(VideoEncoderBackend::Vaapi.encoder_format(), Pixel::VAAPI);
        assert_eq!(
            VideoEncoderBackend::from_name("mpeg4"),
            VideoEncoderBackend::Software
        );
    }

    #[test]
    fn bitrate_only_encoders_are_detected_without_naming_them() {
        // h264_v4l2m2m (and other stateless V4L2 M2M wrappers on e.g.
        // Raspberry Pi) expose no maxrate/bufsize/crf/qp AVOptions - only a
        // plain AVCodecContext::bit_rate is honored. This must be detected
        // by capability probing, not by matching "_v4l2m2m" or any other
        // codec name, so any encoder with the same limitation is covered.
        let Some(codec) = codec::encoder::find_by_name("h264_v4l2m2m") else {
            return;
        };

        assert_eq!(
            VideoEncoderBackend::from_name(codec.name()),
            VideoEncoderBackend::Software
        );
        assert!(video_encoder_is_bitrate_only(codec));

        let cfg = OutputConfig::new(320, 240, 25, 44_100);
        assert!(VideoEncoderBackend::Software.uses_target_bitrate(&cfg, codec));
    }

    #[test]
    fn encoders_with_rate_control_avoptions_are_not_bitrate_only() {
        let codec = codec::encoder::find(codec::Id::H264).expect("libx264 is required for tests");

        assert!(!video_encoder_is_bitrate_only(codec));

        let cfg = OutputConfig::new(320, 240, 25, 44_100);
        assert!(!VideoEncoderBackend::X264.uses_target_bitrate(&cfg, codec));
    }

    #[test]
    fn qsv_input_scaler_allocates_nv12_output() {
        let mut scaler = scaling::Context::get(
            Pixel::YUV420P,
            320,
            240,
            Pixel::NV12,
            320,
            240,
            scaling::flag::Flags::BILINEAR,
        )
        .unwrap();
        let input = frame::Video::new(Pixel::YUV420P, 320, 240);
        let mut output = frame::Video::empty();

        scaler.run(&input, &mut output).unwrap();

        assert_eq!(output.format(), Pixel::NV12);
        assert_eq!((output.width(), output.height()), (320, 240));
    }

    #[test]
    fn vaapi_uses_nv12_before_hardware_upload() {
        let mut scaler = scaling::Context::get(
            Pixel::YUV420P,
            320,
            240,
            VideoEncoderBackend::Vaapi.cpu_input_format(),
            320,
            240,
            scaling::flag::Flags::BILINEAR,
        )
        .unwrap();
        let input = frame::Video::new(Pixel::YUV420P, 320, 240);
        let mut output = frame::Video::empty();

        scaler.run(&input, &mut output).unwrap();

        assert_eq!(output.format(), Pixel::NV12);
    }

    #[test]
    fn qsv_icq_sets_global_quality_on_the_codec_context() {
        let mut cfg = OutputConfig::new(320, 240, 25, 44_100);
        cfg.video_options
            .insert("rate_control".into(), "icq".into());
        cfg.video_options
            .insert("global_quality".into(), "17".into());

        assert!(qsv_uses_icq(&cfg));
        assert_eq!(qsv_global_quality(&cfg), 17);
        assert!(!restore_video_bitrate_metadata(
            VideoEncoderBackend::Qsv,
            &cfg
        ));

        cfg.video_options
            .insert("global_quality".into(), "invalid".into());
        assert_eq!(qsv_global_quality(&cfg), 23);

        cfg.video_options
            .insert("rate_control".into(), "vbr".into());
        assert!(restore_video_bitrate_metadata(
            VideoEncoderBackend::Qsv,
            &cfg
        ));
    }

    #[test]
    fn vaapi_cqp_does_not_restore_bitrate_metadata() {
        let mut cfg = OutputConfig::new(320, 240, 25, 44_100);
        cfg.video_options
            .insert("rate_control".into(), "cqp".into());

        assert!(!restore_video_bitrate_metadata(
            VideoEncoderBackend::Vaapi,
            &cfg
        ));
    }

    #[test]
    fn hls_gop_is_a_short_divisor_of_the_segment_duration() {
        assert_eq!(hls_gop_size(25, 6), 50);
        assert_eq!(hls_gop_size(25, 5), 25);
        assert_eq!(hls_gop_size(30, 4), 60);
        assert_eq!(hls_gop_size(50, 1), 50);
    }

    #[test]
    fn stream_gop_is_two_seconds() {
        assert_eq!(stream_gop_size(25), 50);
        assert_eq!(stream_gop_size(30), 60);
    }
}
