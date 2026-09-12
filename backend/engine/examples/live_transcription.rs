//! Play one media file in the desktop output and render speech recognized by
//! `transcribe-rs` as a live text overlay.
//!
//! The example uses a whisper.cpp GGML model and intentionally keeps the
//! integration small. It is a prototype, not a broadcast subtitle generator:
//! captions appear after the VAD closes an utterance or the eight-second chunk
//! limit is reached.
//!
//! ```text
//! cargo run -p ff-engine --example live_transcription \
//!   --features transcription-example -- video.mp4 models/ggml-base.bin --language en
//! ```

use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread,
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use ff_engine::{
    AudioFrameCallback, ClipResult, OutputConfig, Playout, RgbaColor, TextBackgroundConfig,
    TextConfig, TextOverlayState, TextPosition,
};
use ffmpeg_next::frame;
use transcribe_rs::{
    TranscribeOptions,
    transcriber::{Transcriber, VadChunked, VadChunkedConfig},
    vad::{EnergyVad, SmoothedVad},
    whisper_cpp::WhisperEngine,
};

const PLAYOUT_SAMPLE_RATE: u32 = 48_000;
const TRANSCRIPTION_SAMPLE_RATE: u32 = 16_000;
const DOWNSAMPLE_FACTOR: u8 = (PLAYOUT_SAMPLE_RATE / TRANSCRIPTION_SAMPLE_RATE) as u8;
const AUDIO_QUEUE_FRAMES: usize = 512;

#[derive(Debug, Parser)]
struct Args {
    /// Video or audio file to play
    input: PathBuf,

    /// whisper.cpp GGML model, for example ggml-base.bin
    model: PathBuf,

    /// Optional BCP-47 language hint such as en or de
    #[arg(long)]
    language: Option<String>,

    /// RMS level above which audio is considered speech
    #[arg(long, default_value_t = 0.01)]
    speech_threshold: f32,
}

#[derive(Default)]
struct Downsample48To16 {
    sum: f32,
    count: u8,
}

impl Downsample48To16 {
    fn process(&mut self, input: &frame::Audio) -> Vec<f32> {
        let mut output = Vec::with_capacity(input.samples() / 3 + 1);
        if input.channels() < 2 || input.rate() != PLAYOUT_SAMPLE_RATE {
            return output;
        }

        let left = input.plane::<f32>(0);
        let right = input.plane::<f32>(1);
        for (left, right) in left.iter().zip(right).take(input.samples()) {
            let left = if left.is_finite() { *left } else { 0.0 };
            let right = if right.is_finite() { *right } else { 0.0 };
            self.sum += (left + right) * 0.5;
            self.count += 1;
            if self.count == DOWNSAMPLE_FACTOR {
                output.push(self.sum / f32::from(self.count));
                self.sum = 0.0;
                self.count = 0;
            }
        }
        output
    }
}

fn audio_callback(sender: SyncSender<Vec<f32>>) -> AudioFrameCallback {
    let downsampler = Mutex::new(Downsample48To16::default());
    let queue_full = AtomicBool::new(false);

    AudioFrameCallback::new(move |frame| {
        let samples = downsampler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .process(frame);
        if samples.is_empty() {
            return;
        }

        match sender.try_send(samples) {
            Ok(()) => {
                if queue_full.swap(false, Ordering::Relaxed) {
                    log::info!("transcription audio queue recovered");
                }
            }
            Err(TrySendError::Full(_)) => {
                if !queue_full.swap(true, Ordering::Relaxed) {
                    log::warn!("transcription is slower than playback; dropping audio frames");
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    })
}

fn caption(text: String) -> TextConfig {
    TextConfig {
        text: Some(text),
        font_size: 42.0,
        line_spacing: 6.0,
        position_x: TextPosition::Center,
        position_y: TextPosition::End(48),
        background: Some(TextBackgroundConfig {
            color: RgbaColor {
                r: 0,
                g: 0,
                b: 0,
                a: 190,
            },
            padding: 14,
        }),
        ..TextConfig::default()
    }
}

fn run_transcriber(
    receiver: Receiver<Vec<f32>>,
    overlay: TextOverlayState,
    mut model: WhisperEngine,
    language: Option<String>,
    speech_threshold: f32,
) -> Result<()> {
    let vad = SmoothedVad::new(Box::new(EnergyVad::new(480, speech_threshold)), 10, 15, 2);
    let mut transcriber = VadChunked::new(
        Box::new(vad),
        VadChunkedConfig {
            min_chunk_secs: 1.0,
            max_chunk_secs: 8.0,
            padding_secs: 0.2,
            smart_split_search_secs: Some(2.0),
            merge_separator: " ".to_string(),
        },
        TranscribeOptions {
            language,
            ..TranscribeOptions::default()
        },
    );

    let mut emitted_text = String::new();
    while let Ok(samples) = receiver.recv() {
        for result in transcriber.feed(&mut model, &samples)? {
            let text = result.text.trim();
            if !text.is_empty() {
                println!("{text}");
                overlay.set(Some(caption(text.to_string())));
                if !emitted_text.is_empty() {
                    emitted_text.push(' ');
                }
                emitted_text.push_str(text);
            }
        }
    }

    // Flush an utterance that was still open when playback reached EOF. The
    // returned result contains the complete session, so print only its suffix.
    let final_result = transcriber.finish(&mut model)?;
    let remaining = final_result
        .text
        .strip_prefix(&emitted_text)
        .unwrap_or(&final_result.text)
        .trim();
    if !remaining.is_empty() {
        println!("{remaining}");
    }
    overlay.clear();
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    if !args.speech_threshold.is_finite() || args.speech_threshold < 0.0 {
        return Err(anyhow!("--speech-threshold must be a non-negative number"));
    }

    println!("Loading transcription model {}", args.model.display());
    let model = WhisperEngine::load(&args.model).context("failed to load whisper model")?;
    let overlay = TextOverlayState::default();
    let (audio_sender, audio_receiver) = sync_channel(AUDIO_QUEUE_FRAMES);
    let callback = audio_callback(audio_sender);
    let worker_overlay = overlay.clone();
    let language = args.language.clone();
    let speech_threshold = args.speech_threshold;
    let worker = thread::spawn(move || {
        run_transcriber(
            audio_receiver,
            worker_overlay,
            model,
            language,
            speech_threshold,
        )
    });

    let config = OutputConfig::new(1024, 576, 25, PLAYOUT_SAMPLE_RATE)
        .with_text_overlay_state(overlay)
        .with_audio_frame_callback(Some(callback));
    let mut playout = Playout::open_desktop(config, 10.0)?;
    let input = args.input.to_string_lossy();
    match playout.play(input.as_ref())? {
        ClipResult::Played | ClipResult::Skipped | ClipResult::LiveEnded => {}
        ClipResult::Fallback { reason } => {
            log::error!("playback failed and generated fallback: {reason}");
        }
        ClipResult::Stopped => {}
    }
    playout.finish()?;

    worker
        .join()
        .map_err(|_| anyhow!("transcription worker panicked"))??;
    Ok(())
}
