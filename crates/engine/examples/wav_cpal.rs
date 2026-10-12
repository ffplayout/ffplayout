//! Minimal WAV-to-CPAL playback diagnostic.
//!
//! Usage:
//!   cargo run -p ff-engine --example wav_cpal -- /path/to/input.wav
//!
//! The example deliberately has no ffplayout mixer, resampler or renderer in
//! its path. It accepts PCM signed 16/24/32-bit and IEEE-float 32-bit WAVs.

use std::{
    env, fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cpal::{
    FromSample, I24, Sample, SampleFormat, SizedSample, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

struct Wav {
    sample_rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

struct PlaybackSamples {
    samples: Arc<[f32]>,
    input_channels: usize,
    next_frame: AtomicUsize,
}

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: wav_cpal <input.wav>"))?;
    let wav = read_wav(&path)?;
    let frames = wav.samples.len() / wav.channels;
    let duration = Duration::from_secs_f64(frames as f64 / f64::from(wav.sample_rate));

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default audio output device"))?;
    let supported = device
        .supported_output_configs()
        .context("querying supported output configurations")?
        .filter(|config| {
            config.channels() == 2
                && config.min_sample_rate() <= wav.sample_rate
                && config.max_sample_rate() >= wav.sample_rate
        })
        .max_by_key(|config| sample_format_quality(config.sample_format()))
        .ok_or_else(|| {
            anyhow!(
                "default output device does not support {} Hz",
                wav.sample_rate
            )
        })?
        .with_sample_rate(wav.sample_rate);
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let output_channels = config.channels as usize;
    println!(
        "playing {path}: {} Hz, {} channel(s) -> default output: {:?}, {} channel(s)",
        wav.sample_rate, wav.channels, format, output_channels
    );

    let samples = Arc::new(PlaybackSamples {
        samples: Arc::from(wav.samples),
        input_channels: wav.channels,
        next_frame: AtomicUsize::new(0),
    });
    let stream = match format {
        SampleFormat::I8 => build_stream::<i8>(&device, &config, samples)?,
        SampleFormat::U8 => build_stream::<u8>(&device, &config, samples)?,
        SampleFormat::I16 => build_stream::<i16>(&device, &config, samples)?,
        SampleFormat::U16 => build_stream::<u16>(&device, &config, samples)?,
        SampleFormat::I24 => build_stream::<I24>(&device, &config, samples)?,
        SampleFormat::U24 => build_stream::<U24>(&device, &config, samples)?,
        SampleFormat::I32 => build_stream::<i32>(&device, &config, samples)?,
        SampleFormat::U32 => build_stream::<u32>(&device, &config, samples)?,
        SampleFormat::I64 => build_stream::<i64>(&device, &config, samples)?,
        SampleFormat::U64 => build_stream::<u64>(&device, &config, samples)?,
        SampleFormat::F32 => build_stream::<f32>(&device, &config, samples)?,
        SampleFormat::F64 => build_stream::<f64>(&device, &config, samples)?,
        other => return Err(anyhow!("unsupported output sample format: {other:?}")),
    };
    stream.play().context("starting audio output")?;
    thread::sleep(duration + Duration::from_millis(250));
    Ok(())
}

fn sample_format_quality(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 100,
        SampleFormat::F64 => 90,
        SampleFormat::I32 | SampleFormat::U32 => 80,
        SampleFormat::I24 | SampleFormat::U24 => 70,
        SampleFormat::I16 | SampleFormat::U16 => 60,
        SampleFormat::I8 | SampleFormat::U8 => 10,
        _ => 0,
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<PlaybackSamples>,
) -> Result<cpal::Stream>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let output_channels = config.channels as usize;
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| {
                for frame in output.chunks_mut(output_channels) {
                    let source_frame = samples.next_frame.fetch_add(1, Ordering::Relaxed);
                    let offset = source_frame.saturating_mul(samples.input_channels);
                    let left = samples.samples.get(offset).copied().unwrap_or(0.0);
                    let right = samples
                        .samples
                        .get(offset + (samples.input_channels > 1) as usize)
                        .copied()
                        .unwrap_or(left);
                    for (channel, output) in frame.iter_mut().enumerate() {
                        let sample = match channel {
                            0 => left,
                            1 => right,
                            _ => 0.0,
                        };
                        *output = T::from_sample(sample);
                    }
                }
            },
            |error| log::warn!("CPAL output error: {error}"),
            None,
        )
        .context("building audio output stream")
}

fn read_wav(path: &str) -> Result<Wav> {
    let data = fs::read(path).with_context(|| format!("reading {path}"))?;
    if data.get(0..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WAVE") {
        return Err(anyhow!("not a RIFF/WAVE file"));
    }
    let mut offset = 12;
    let mut format = None;
    let mut audio = None;
    while offset + 8 <= data.len() {
        let id = &data[offset..offset + 4];
        let size = u32::from_le_bytes(data[offset + 4..offset + 8].try_into()?) as usize;
        let start = offset + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| anyhow!("invalid WAV chunk"))?;
        if end > data.len() {
            return Err(anyhow!("truncated WAV chunk"));
        }
        if id == b"fmt " {
            format = Some(&data[start..end]);
        }
        if id == b"data" {
            audio = Some(&data[start..end]);
        }
        offset = end + (size % 2);
    }
    let format = format.ok_or_else(|| anyhow!("WAV has no fmt chunk"))?;
    if format.len() < 16 {
        return Err(anyhow!("invalid WAV fmt chunk"));
    }
    let code = u16::from_le_bytes(format[0..2].try_into()?);
    let channels = u16::from_le_bytes(format[2..4].try_into()?) as usize;
    let sample_rate = u32::from_le_bytes(format[4..8].try_into()?);
    let bits = u16::from_le_bytes(format[14..16].try_into()?);
    let audio = audio.ok_or_else(|| anyhow!("WAV has no data chunk"))?;
    if channels == 0 || !matches!((code, bits), (1, 16 | 24 | 32) | (3, 32)) {
        return Err(anyhow!("unsupported WAV format: code={code}, bits={bits}"));
    }
    let bytes = usize::from(bits / 8);
    if audio.len() % bytes != 0 {
        return Err(anyhow!("truncated WAV samples"));
    }
    let samples = audio
        .chunks_exact(bytes)
        .map(|raw| match (code, bits) {
            (1, 16) => i16::from_le_bytes(raw.try_into().expect("exact sample")) as f32 / 32768.0,
            (1, 24) => {
                let value = i32::from_le_bytes([
                    raw[0],
                    raw[1],
                    raw[2],
                    if raw[2] & 0x80 != 0 { 0xff } else { 0 },
                ]);
                value as f32 / 8_388_608.0
            }
            (1, 32) => {
                i32::from_le_bytes(raw.try_into().expect("exact sample")) as f32 / 2_147_483_648.0
            }
            (3, 32) => f32::from_le_bytes(raw.try_into().expect("exact sample")),
            _ => unreachable!(),
        })
        .collect();
    Ok(Wav {
        sample_rate,
        channels,
        samples,
    })
}
