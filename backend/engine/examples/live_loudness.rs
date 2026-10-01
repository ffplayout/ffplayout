//! Offline reference runner for the engine's live loudness processor.
//!
//! ```text
//! cargo run -p ff-engine --example live_loudness -- INPUT_FILE --target-lufs -17 --max-gain-db 16
//! ```
//!
//! When present, the video stream is copied into an MP4. The first audio stream
//! is decoded, normalized by [`LiveLoudnessProcessor`], and encoded as 128
//! kbit/s Opus. Audio-only inputs produce an `.opus` file. No external `ffmpeg`
//! executable is used. With `--desktop`, audio and video are processed directly
//! during playback, buffering only the selected lookahead (requires a desktop feature).

use std::{
    collections::VecDeque,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use ff_engine::{
    BufferedLoudnessAnalysis, LiveDynamicsProcessor, LiveLoudnessConfig, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor,
};
use ffmpeg::Rescale;
use ffmpeg::{
    Stream, codec, encoder, format, frame, media,
    rescale::TIME_BASE,
    software::resampling,
    util::{
        channel_layout::ChannelLayout,
        format::sample::{Sample, Type},
    },
};
use ffmpeg_next as ffmpeg;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum AnalysisMode {
    /// Use the live short-term rider with 10 ms peak lookahead.
    #[default]
    ImmediateShortTerm,
    /// Buffer 500 ms and drive the rider from EBU R128 momentary loudness.
    Momentary500ms,
    /// Buffer 3 seconds and drive the rider from EBU R128 short-term loudness.
    ShortTerm3s,
    /// Experimental slow AGC, 50 ms compressor preview and true-peak limiter.
    LiveDynamics,
}

impl AnalysisMode {
    fn file_suffix(self) -> &'static str {
        match self {
            Self::ImmediateShortTerm => "immediate-short-term",
            Self::Momentary500ms => "momentary-500ms",
            Self::ShortTerm3s => "short-term-3s",
            Self::LiveDynamics => "live-dynamics",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    about = "Normalize an audio stream with the engine's live loudness processor",
    after_help = "All modes use the live 10 ms true-peak limiter. The immediate mode uses causal short-term analysis. Offline buffering preserves audio/video alignment. The buffered rider modes preserve source timing. live-dynamics adds a stereo-linked 6:1 soft-knee compressor with 50 ms preview, slow AGC, and a -55 dBFS pause detector. It needs about 70 ms of audio preview, not three seconds.\n\nFor unusually quiet source material, a target of -17 LUFS may require more than the conservative default of --max-gain-db 8; for example use --max-gain-db 16."
)]
struct Arguments {
    /// Input audio or video file. Video inputs retain their original video stream.
    input: PathBuf,

    /// Play directly in a desktop window with audio, without rendering a file.
    #[cfg(feature = "desktop-base")]
    #[arg(long)]
    desktop: bool,

    /// Hide the audio statistics overlay during desktop playback.
    #[cfg(feature = "desktop-base")]
    #[arg(long)]
    no_audio_stats: bool,

    /// Overwrite an existing output file.
    #[arg(short = 'y', long)]
    overwrite: bool,

    /// Loudness analysis/correction mode. Offline buffering preserves A/V alignment; every mode uses the live peak limiter. Options are: immediate-short-term, momentary500ms, short-term3s, live-dynamics.
    #[arg(long, value_enum, default_value_t = AnalysisMode::ImmediateShortTerm)]
    analysis_mode: AnalysisMode,

    /// Target loudness in LUFS. Higher values are louder; e.g. -17 is louder than -23.
    #[arg(short = 't', long, default_value_t = -23.0, allow_hyphen_values = true)]
    target_lufs: f64,

    /// No gain change inside this distance from the target, in LU. A wider band reduces gain movement.
    #[arg(long, default_value_t = 1.0)]
    dead_band_lu: f64,

    /// Largest upward gain the live rider may apply, in dB. Increase for very quiet sources; higher values also raise background noise.
    #[arg(long, default_value_t = 8.0)]
    max_gain_db: f64,

    /// Largest attenuation the live rider may apply, in dB. This must be zero or negative; e.g. -12 allows reducing loud input by up to 12 dB.
    #[arg(long, default_value_t = -12.0, allow_hyphen_values = true)]
    max_attenuation_db: f64,

    /// Maximum upward gain change per second, in dB/s. Smaller values avoid pumping but take longer to lift quiet material.
    #[arg(long, default_value_t = 0.5)]
    gain_up_db_per_second: f64,

    /// Maximum downward gain change per second, in dB/s. A higher value reacts more quickly to unexpectedly loud input.
    #[arg(long, default_value_t = 2.0)]
    gain_down_db_per_second: f64,

    /// Silence gate for rider modes, in LUFS. live-dynamics uses a fixed -55 dBFS pause threshold.
    #[arg(long, default_value_t = -60.0, allow_hyphen_values = true)]
    silence_gate_lufs: f64,

    /// Final true-peak ceiling in dBTP, applied by the live lookahead limiter.
    #[arg(long, default_value_t = -1.0, allow_hyphen_values = true)]
    true_peak_ceiling_dbtp: f64,
}

impl Arguments {
    fn loudness_config(&self) -> Result<LiveLoudnessConfig> {
        let values = [
            self.target_lufs,
            self.dead_band_lu,
            self.max_gain_db,
            self.max_attenuation_db,
            self.gain_up_db_per_second,
            self.gain_down_db_per_second,
            self.silence_gate_lufs,
            self.true_peak_ceiling_dbtp,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            bail!("all loudness parameters must be finite numbers");
        }
        if self.dead_band_lu < 0.0
            || self.max_gain_db < 0.0
            || self.max_attenuation_db > 0.0
            || self.gain_up_db_per_second < 0.0
            || self.gain_down_db_per_second < 0.0
        {
            bail!(
                "gain limits and rates must be positive; max attenuation must be zero or negative"
            );
        }
        Ok(LiveLoudnessConfig {
            target_lufs: self.target_lufs,
            dead_band_lu: self.dead_band_lu,
            max_gain_db: self.max_gain_db,
            max_attenuation_db: self.max_attenuation_db,
            gain_up_db_per_second: self.gain_up_db_per_second,
            gain_down_db_per_second: self.gain_down_db_per_second,
            silence_gate_lufs: self.silence_gate_lufs,
            true_peak_ceiling_dbtp: self.true_peak_ceiling_dbtp,
            ..LiveLoudnessConfig::default()
        })
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let arguments = Arguments::parse();
    let input = arguments.input.clone();
    let loudness_config = arguments.loudness_config()?;
    ffmpeg::init().context("initializing FFmpeg libraries")?;
    #[cfg(feature = "desktop-base")]
    if arguments.desktop {
        return play_desktop(&arguments, loudness_config);
    }

    let mut input_context = format::input(&input).context("opening input")?;
    let video_input = input_context.streams().best(media::Type::Video);
    let audio_input = input_context
        .streams()
        .best(media::Type::Audio)
        .context("input has no audio stream")?;
    let video_index = video_input.as_ref().map(Stream::index);
    let audio_index = audio_input.index();
    let audio_time_base = audio_input.time_base();
    let output = output_path(&input, video_input.is_some(), arguments.analysis_mode)?;
    if output.exists() && !arguments.overwrite {
        bail!("output file already exists: {}", output.display());
    }
    let mut progress = Progress::new(input_context.duration());

    let audio_decoder_context = codec::context::Context::from_parameters(audio_input.parameters())?;
    let mut audio_decoder = audio_decoder_context.decoder().audio()?;
    let input_layout = channel_layout(&audio_decoder);
    let mut decode_resampler = resampling::Context::get(
        audio_decoder.format(),
        input_layout,
        audio_decoder.rate(),
        Sample::F32(Type::Planar),
        ChannelLayout::STEREO,
        SAMPLE_RATE,
    )?;

    let mut output_context = format::output(&output).context("creating output")?;
    let video_output_index = video_input
        .map(|video_input| {
            let mut stream = output_context.add_stream(encoder::find(codec::Id::None))?;
            stream.set_parameters(video_input.parameters());
            // The input tag can be invalid in an MP4 stream-copy output.
            unsafe {
                (*stream.parameters().as_mut_ptr()).codec_tag = 0;
            }
            stream.set_time_base(video_input.time_base());
            Ok::<_, anyhow::Error>(stream.index())
        })
        .transpose()?;

    let opus = codec::encoder::find_by_name("libopus").context("libopus encoder is unavailable")?;
    let global_header = output_context
        .format()
        .flags()
        .contains(format::flag::Flags::GLOBAL_HEADER);
    let encoder_format = preferred_audio_format(opus)?;
    let (audio_output_index, mut audio_encoder) = {
        let mut stream = output_context.add_stream(opus)?;
        let mut context = codec::context::Context::new_with_codec(opus)
            .encoder()
            .audio()?;
        context.set_rate(SAMPLE_RATE as i32);
        context.set_channel_layout(ChannelLayout::STEREO);
        context.set_format(encoder_format);
        context.set_time_base((1, SAMPLE_RATE as i32));
        context.set_bit_rate(128_000);
        if global_header {
            context.set_flags(codec::flag::Flags::GLOBAL_HEADER);
        }
        let encoder = context.open_as(opus)?;
        stream.set_parameters(&encoder);
        stream.set_time_base((1, SAMPLE_RATE as i32));
        (stream.index(), encoder)
    };
    let mut encode_resampler = (encoder_format != Sample::F32(Type::Planar))
        .then(|| {
            resampling::Context::get(
                Sample::F32(Type::Planar),
                ChannelLayout::STEREO,
                SAMPLE_RATE,
                encoder_format,
                ChannelLayout::STEREO,
                SAMPLE_RATE,
            )
        })
        .transpose()?;

    output_context.set_metadata(input_context.metadata().to_owned());
    output_context.write_header()?;

    let mut loudness = LoudnessPipeline::new(loudness_config, arguments.analysis_mode)?;
    let frame_size = audio_encoder.frame_size() as usize;
    if frame_size == 0 {
        bail!("libopus reported a zero audio frame size");
    }
    let mut samples = [Vec::new(), Vec::new()];
    let mut next_audio_pts = 0_i64;
    progress.print();

    for (stream, mut packet) in input_context.packets() {
        match stream.index() {
            index if video_index == Some(index) => {
                let video_output_index =
                    video_output_index.context("video output stream missing")?;
                progress.report(packet.pts().or_else(|| packet.dts()), stream.time_base());
                let time_base = output_context
                    .stream(video_output_index)
                    .context("video output stream missing")?
                    .time_base();
                packet.rescale_ts(stream.time_base(), time_base);
                packet.set_position(-1);
                packet.set_stream(video_output_index);
                packet.write_interleaved(&mut output_context)?;
            }
            index if index == audio_index => {
                if video_index.is_none() {
                    progress.report(packet.pts().or_else(|| packet.dts()), audio_time_base);
                }
                audio_decoder.send_packet(&packet)?;
                decode_and_normalize(
                    &mut audio_decoder,
                    &mut decode_resampler,
                    &mut loudness,
                    &mut samples,
                )?;
                write_ready_audio(
                    &mut samples,
                    frame_size,
                    &mut next_audio_pts,
                    &mut audio_encoder,
                    &mut encode_resampler,
                    &mut output_context,
                    audio_output_index,
                )?;
            }
            _ => {}
        }
    }

    audio_decoder.send_eof()?;
    decode_and_normalize(
        &mut audio_decoder,
        &mut decode_resampler,
        &mut loudness,
        &mut samples,
    )?;
    flush_decode_resampler(&mut decode_resampler, &mut loudness, &mut samples)?;
    loudness.flush(&mut samples);
    write_ready_audio(
        &mut samples,
        frame_size,
        &mut next_audio_pts,
        &mut audio_encoder,
        &mut encode_resampler,
        &mut output_context,
        audio_output_index,
    )?;
    if !samples[0].is_empty() {
        for channel in &mut samples {
            channel.resize(frame_size, 0.0);
        }
        write_ready_audio(
            &mut samples,
            frame_size,
            &mut next_audio_pts,
            &mut audio_encoder,
            &mut encode_resampler,
            &mut output_context,
            audio_output_index,
        )?;
    }
    flush_encode_resampler(
        &mut encode_resampler,
        &mut audio_encoder,
        &mut output_context,
        audio_output_index,
    )?;
    audio_encoder.send_eof()?;
    write_encoded_packets(&mut audio_encoder, &mut output_context, audio_output_index)?;
    output_context.write_trailer()?;

    progress.finish();
    println!("created: {}", output.display());
    println!("final metrics: {:#?}", loudness.metrics());

    Ok(())
}

#[cfg(feature = "desktop-base")]
fn play_desktop(arguments: &Arguments, loudness: LiveLoudnessConfig) -> Result<()> {
    use std::time::Duration;

    use ff_engine::{ClipResult, OutputConfig, Playout};

    let (measurement, lookahead) = match arguments.analysis_mode {
        AnalysisMode::ImmediateShortTerm => (LiveLoudnessMeasurement::ShortTerm, Duration::ZERO),
        AnalysisMode::Momentary500ms => (
            LiveLoudnessMeasurement::Momentary,
            Duration::from_millis(500),
        ),
        AnalysisMode::ShortTerm3s => (LiveLoudnessMeasurement::ShortTerm, Duration::from_secs(3)),
        AnalysisMode::LiveDynamics => (LiveLoudnessMeasurement::ShortTerm, Duration::ZERO),
    };
    let config = OutputConfig::new(1280, 720, 25, SAMPLE_RATE);
    let mut playout = Playout::open_desktop(config, 10.0)?;
    let (result, metrics) = playout.play_with_loudness_preview(
        arguments.input.to_string_lossy().as_ref(),
        loudness,
        measurement,
        lookahead,
        matches!(arguments.analysis_mode, AnalysisMode::LiveDynamics),
        !arguments.no_audio_stats,
    )?;

    if let ClipResult::Fallback { reason } = &result {
        bail!("desktop playback failed: {reason}");
    }

    playout.finish()?;
    if !matches!(result, ClipResult::Stopped) {
        println!("final metrics: {metrics:#?}");
    }

    Ok(())
}

struct Progress {
    duration_us: Option<i64>,
    latest_us: i64,
    last_reported_us: i64,
}

impl Progress {
    fn new(duration_us: i64) -> Self {
        Self {
            duration_us: (duration_us > 0).then_some(duration_us),
            latest_us: 0,
            last_reported_us: 0,
        }
    }

    fn report(&mut self, timestamp: Option<i64>, time_base: ffmpeg::Rational) {
        let Some(timestamp) = timestamp else {
            return;
        };
        self.latest_us = self.latest_us.max(timestamp.rescale(time_base, TIME_BASE));
        if self.latest_us - self.last_reported_us >= 1_000_000 {
            self.print();
            self.last_reported_us = self.latest_us;
        }
    }

    fn finish(&mut self) {
        if let Some(duration_us) = self.duration_us {
            self.latest_us = duration_us;
        }
        self.print();
        println!();
    }

    fn print(&self) {
        let seconds = self.latest_us as f64 / 1_000_000.0;
        if let Some(duration_us) = self.duration_us {
            let percent = (self.latest_us as f64 / duration_us as f64 * 100.0).min(100.0);
            print!("\rprocessing: {seconds:.1} s ({percent:.0}%)");
        } else {
            print!("\rprocessing: {seconds:.1} s");
        }
        let _ = io::stdout().flush();
    }
}

struct LoudnessPipeline {
    processor: LiveLoudnessProcessor,
    dynamics: Option<LiveDynamicsProcessor>,
    lookahead_samples: usize,
    buffered_samples: usize,
    pending: VecDeque<(frame::Audio, Option<BufferedLoudnessAnalysis>)>,
}

impl LoudnessPipeline {
    fn new(config: LiveLoudnessConfig, mode: AnalysisMode) -> Result<Self> {
        let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config)?;
        let lookahead_samples = match mode {
            AnalysisMode::ImmediateShortTerm => 0,
            AnalysisMode::Momentary500ms => {
                processor.set_measurement(LiveLoudnessMeasurement::Momentary);
                SAMPLE_RATE as usize / 2
            }
            AnalysisMode::ShortTerm3s => SAMPLE_RATE as usize * 3,
            AnalysisMode::LiveDynamics => 0,
        };
        let dynamics = matches!(mode, AnalysisMode::LiveDynamics)
            .then(|| LiveDynamicsProcessor::new(SAMPLE_RATE, config))
            .transpose()?;

        Ok(Self {
            processor,
            dynamics,
            lookahead_samples,
            buffered_samples: 0,
            pending: VecDeque::new(),
        })
    }

    fn process(&mut self, mut frame: frame::Audio, samples: &mut [Vec<f32>; CHANNELS]) {
        let analysis =
            (self.lookahead_samples > 0).then(|| self.processor.analyze_buffered(&mut frame));

        self.buffered_samples += frame.samples();
        self.pending.push_back((frame, analysis));
        let preview_samples = self.lookahead_samples.max(self.required_samples());

        while self.pending.front().is_some_and(|(frame, _)| {
            self.buffered_samples.saturating_sub(frame.samples()) >= preview_samples
        }) {
            self.emit_front(samples);
        }
    }

    fn emit_front(&mut self, samples: &mut [Vec<f32>; CHANNELS]) {
        let Some((mut frame, analysis)) = self.pending.pop_front() else {
            return;
        };
        self.buffered_samples -= frame.samples();
        let limit = self.required_samples();
        let mut future = Vec::with_capacity(limit);

        for (buffered, _) in &self.pending {
            for index in 0..buffered.samples() {
                if future.len() == limit {
                    break;
                }

                future.push([
                    buffered.plane::<f32>(0)[index],
                    buffered.plane::<f32>(1)[index],
                ]);
            }

            if future.len() == limit {
                break;
            }
        }

        if let Some(dynamics) = self.dynamics.as_mut() {
            dynamics.process(&mut frame, &future);
        } else if let Some(analysis) = analysis {
            self.processor
                .apply_buffered_gain(&mut frame, &analysis, &future);
        } else {
            self.processor.process_with_lookahead(&mut frame, &future);
        }

        append_frame(samples, &frame);
    }

    fn flush(&mut self, samples: &mut [Vec<f32>; CHANNELS]) {
        while !self.pending.is_empty() {
            self.emit_front(samples);
        }
    }

    fn required_samples(&self) -> usize {
        self.dynamics.as_ref().map_or(
            self.processor.peak_lookahead_samples(),
            LiveDynamicsProcessor::lookahead_samples,
        )
    }

    fn metrics(&self) -> LiveLoudnessMetrics {
        self.dynamics
            .as_ref()
            .map_or_else(|| self.processor.metrics(), LiveDynamicsProcessor::metrics)
    }
}

fn append_frame(samples: &mut [Vec<f32>; CHANNELS], frame: &frame::Audio) {
    for (channel, buffer) in samples.iter_mut().enumerate() {
        buffer.extend_from_slice(frame.plane::<f32>(channel));
    }
}

fn decode_and_normalize(
    decoder: &mut codec::decoder::Audio,
    resampler: &mut resampling::Context,
    loudness: &mut LoudnessPipeline,
    samples: &mut [Vec<f32>; CHANNELS],
) -> Result<()> {
    let mut decoded = frame::Audio::empty();
    while decoder.receive_frame(&mut decoded).is_ok() {
        if decoded.channel_layout().is_empty() {
            decoded.set_channel_layout(channel_layout(decoder));
        }
        let converted = resample_frame(resampler, &decoded)?;
        loudness.process(converted, samples);
    }
    Ok(())
}

fn flush_decode_resampler(
    resampler: &mut resampling::Context,
    loudness: &mut LoudnessPipeline,
    samples: &mut [Vec<f32>; CHANNELS],
) -> Result<()> {
    loop {
        let mut converted =
            frame::Audio::new(Sample::F32(Type::Planar), 4096, ChannelLayout::STEREO);
        converted.set_rate(SAMPLE_RATE);
        if resampler.flush(&mut converted)?.is_none() || converted.samples() == 0 {
            return Ok(());
        }
        loudness.process(converted, samples);
    }
}

fn write_ready_audio(
    samples: &mut [Vec<f32>; CHANNELS],
    frame_size: usize,
    next_pts: &mut i64,
    encoder: &mut codec::encoder::Audio,
    resampler: &mut Option<resampling::Context>,
    output: &mut format::context::Output,
    output_index: usize,
) -> Result<()> {
    while samples.iter().all(|channel| channel.len() >= frame_size) {
        let mut audio =
            frame::Audio::new(Sample::F32(Type::Planar), frame_size, ChannelLayout::STEREO);
        audio.set_rate(SAMPLE_RATE);
        audio.set_pts(Some(*next_pts));
        for (channel, buffer) in samples.iter_mut().enumerate() {
            audio
                .plane_mut::<f32>(channel)
                .copy_from_slice(&buffer[..frame_size]);
            buffer.drain(..frame_size);
        }
        *next_pts += frame_size as i64;
        if let Some(resampler) = resampler {
            let mut converted = frame::Audio::empty();
            resampler.run(&audio, &mut converted)?;
            converted.set_pts(audio.pts());
            encoder.send_frame(&converted)?;
        } else {
            encoder.send_frame(&audio)?;
        }
        write_encoded_packets(encoder, output, output_index)?;
    }
    Ok(())
}

fn flush_encode_resampler(
    resampler: &mut Option<resampling::Context>,
    encoder: &mut codec::encoder::Audio,
    output: &mut format::context::Output,
    output_index: usize,
) -> Result<()> {
    let Some(resampler) = resampler else {
        return Ok(());
    };
    loop {
        let definition = *resampler.output();
        let mut converted = frame::Audio::new(definition.format, 4096, definition.channel_layout);
        converted.set_rate(definition.rate);
        if resampler.flush(&mut converted)?.is_none() || converted.samples() == 0 {
            return Ok(());
        }
        encoder.send_frame(&converted)?;
        write_encoded_packets(encoder, output, output_index)?;
    }
}

fn write_encoded_packets(
    encoder: &mut codec::encoder::Audio,
    output: &mut format::context::Output,
    output_index: usize,
) -> Result<()> {
    let time_base = output
        .stream(output_index)
        .context("audio output stream missing")?
        .time_base();
    let mut packet = ffmpeg::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(output_index);
        packet.rescale_ts(encoder.time_base(), time_base);
        packet.write_interleaved(output)?;
    }
    Ok(())
}

fn resample_frame(
    resampler: &mut resampling::Context,
    input: &frame::Audio,
) -> Result<frame::Audio> {
    let capacity =
        unsafe { ffmpeg::ffi::swr_get_out_samples(resampler.as_mut_ptr(), input.samples() as i32) };
    if capacity < 0 {
        return Err(ffmpeg::Error::from(capacity)).context("calculating resampler capacity");
    }
    let definition = *resampler.output();
    let mut output = frame::Audio::new(
        definition.format,
        capacity.max(1) as usize,
        definition.channel_layout,
    );
    output.set_rate(definition.rate);
    resampler.run(input, &mut output)?;
    Ok(output)
}

fn channel_layout(decoder: &codec::decoder::Audio) -> ChannelLayout {
    let layout = decoder.channel_layout();
    if layout.is_empty() {
        ChannelLayout::default(i32::from(decoder.channels()).max(1))
    } else {
        layout
    }
}

fn preferred_audio_format(codec: codec::codec::Codec) -> Result<Sample> {
    let preferred = Sample::F32(Type::Planar);
    let formats: Vec<_> = codec
        .audio()?
        .formats()
        .context("libopus exposes no sample formats")?
        .collect();
    formats
        .iter()
        .copied()
        .find(|format| *format == preferred)
        .or_else(|| formats.first().copied())
        .context("libopus exposes an empty sample format list")
}

fn output_path(input: &Path, has_video: bool, analysis_mode: AnalysisMode) -> Result<PathBuf> {
    if !input.is_file() {
        bail!("input file not found: {}", input.display());
    }
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input
        .file_stem()
        .context("input file has no name")?
        .to_string_lossy();
    let extension = if has_video { "mp4" } else { "opus" };
    Ok(parent.join(format!(
        "{stem} # live_loudness # {}.{extension}",
        analysis_mode.file_suffix()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: usize, value: f32) -> frame::Audio {
        let mut frame =
            frame::Audio::new(Sample::F32(Type::Planar), samples, ChannelLayout::STEREO);
        frame.set_rate(SAMPLE_RATE);

        for channel in 0..CHANNELS {
            frame.plane_mut::<f32>(channel).fill(value);
        }

        frame
    }

    #[test]
    fn all_modes_limit_future_peaks_and_drain_without_losing_samples() {
        for mode in [
            AnalysisMode::ImmediateShortTerm,
            AnalysisMode::Momentary500ms,
            AnalysisMode::ShortTerm3s,
            AnalysisMode::LiveDynamics,
        ] {
            let mut pipeline = LoudnessPipeline::new(LiveLoudnessConfig::default(), mode).unwrap();
            let mut output = [Vec::new(), Vec::new()];
            pipeline.process(audio(1_024, 0.1), &mut output);
            assert!(output[0].is_empty());
            let mut peak = audio(1_024, 0.1);

            for channel in 0..CHANNELS {
                peak.plane_mut::<f32>(channel)[0] = 2.0;
            }

            pipeline.process(peak, &mut output);

            if pipeline.lookahead_samples > 0 {
                assert!(output[0].is_empty());
            }

            pipeline.flush(&mut output);
            assert_eq!(output[0].len(), 2_048);
            assert_eq!(output[0], output[1]);
            assert!(output[0][1_000] < 0.06);
            assert!(
                output[0]
                    .iter()
                    .all(|value| value.abs() <= 10.0_f32.powf(-1.0 / 20.0))
            );
            assert_eq!(pipeline.buffered_samples, 0);
        }
    }

    #[test]
    fn three_second_mode_analyzes_future_audio_before_emitting() {
        let mut pipeline =
            LoudnessPipeline::new(LiveLoudnessConfig::default(), AnalysisMode::ShortTerm3s)
                .unwrap();
        let mut output = [Vec::new(), Vec::new()];

        for _ in 0..30 {
            pipeline.process(audio(4_800, 0.1), &mut output);
        }

        assert!(output[0].is_empty());
        assert!(pipeline.metrics().short_term_lufs.is_some());
        pipeline.process(audio(4_800, 0.1), &mut output);
        assert_eq!(output[0].len(), 4_800);
        pipeline.flush(&mut output);
        assert_eq!(output[0].len(), 31 * 4_800);
    }
    #[test]
    fn three_second_buffer_keeps_gain_changes_on_the_source_timeline() {
        let mut immediate = LoudnessPipeline::new(
            LiveLoudnessConfig::default(),
            AnalysisMode::ImmediateShortTerm,
        )
        .unwrap();
        let mut delayed =
            LoudnessPipeline::new(LiveLoudnessConfig::default(), AnalysisMode::ShortTerm3s)
                .unwrap();
        let mut reference = [Vec::new(), Vec::new()];
        let mut output = [Vec::new(), Vec::new()];
        let rate = SAMPLE_RATE as usize;

        // Irregular packet boundaries cross the 100 ms measurement cadence.
        // Loud input at four seconds must not attenuate the quiet input at one second.
        for offset in (0..rate * 10).step_by(777) {
            let count = 777.min(rate * 10 - offset);
            let mut frame = audio(count, 0.0);

            for channel in 0..CHANNELS {
                for (index, sample) in frame.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    let position = offset + index;
                    let amplitude = if (rate * 4..rate * 6).contains(&position) {
                        0.4
                    } else {
                        0.01
                    };
                    *sample = (std::f64::consts::TAU * 997.0 * position as f64
                        / f64::from(SAMPLE_RATE))
                    .sin() as f32
                        * amplitude;
                }
            }

            immediate.process(frame.clone(), &mut reference);
            delayed.process(frame, &mut output);
        }

        immediate.flush(&mut reference);
        delayed.flush(&mut output);
        assert_eq!(output[0].len(), rate * 10);

        for (index, (&actual, &expected)) in output[0].iter().zip(&reference[0]).enumerate() {
            assert!(
                (actual - expected).abs() < 0.000001,
                "gain correction shifted at sample {index}: {actual} vs {expected}"
            );
        }
    }
}
