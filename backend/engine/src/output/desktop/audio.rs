use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use cpal::{
    FromSample, I24, Sample, SampleFormat, SizedSample, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use super::{AUDIO_CHANNELS, AUDIO_DEVICE_BUFFER_SAMPLES, AUDIO_MAX_QUEUE_MS};

pub(super) struct DesktopAudio {
    _stream: cpal::Stream,
    state: Arc<AudioState>,
    device_buffer_samples: u64,
}

struct AudioState {
    samples: Mutex<VecDeque<f32>>,
    queued_samples: AtomicU64,
    playing: AtomicBool,
}

impl DesktopAudio {
    pub(super) fn open(sample_rate: u32) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default audio output device"))?;
        let supported = device
            .supported_output_configs()
            .context("querying supported audio output configurations")?
            .find(|config| {
                config.channels() as usize == AUDIO_CHANNELS
                    && config.min_sample_rate() <= sample_rate
                    && config.max_sample_rate() >= sample_rate
            })
            .ok_or_else(|| {
                anyhow!("no stereo audio output configuration supports {sample_rate} Hz")
            })?
            .with_sample_rate(sample_rate);
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let device_buffer_samples = match config.buffer_size {
            cpal::BufferSize::Fixed(size) => u64::from(size),
            cpal::BufferSize::Default => AUDIO_DEVICE_BUFFER_SAMPLES,
        };
        let state = Arc::new(AudioState {
            samples: Mutex::new(VecDeque::with_capacity(
                (sample_rate as usize * AUDIO_MAX_QUEUE_MS as usize / 1_000) * AUDIO_CHANNELS,
            )),
            queued_samples: AtomicU64::new(0),
            playing: AtomicBool::new(false),
        });
        let callback_state = Arc::clone(&state);
        let error_callback = |error| log::warn!("desktop audio stream error: {error}");
        let stream = match sample_format {
            SampleFormat::I8 => {
                build_audio_stream::<i8>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::U8 => {
                build_audio_stream::<u8>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::I16 => {
                build_audio_stream::<i16>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::U16 => {
                build_audio_stream::<u16>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::I24 => {
                build_audio_stream::<I24>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::U24 => {
                build_audio_stream::<U24>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::I32 => {
                build_audio_stream::<i32>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::U32 => {
                build_audio_stream::<u32>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::I64 => {
                build_audio_stream::<i64>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::U64 => {
                build_audio_stream::<u64>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::F32 => {
                build_audio_stream::<f32>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::F64 => {
                build_audio_stream::<f64>(&device, &config, callback_state, error_callback)?
            }
            SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32 => {
                return Err(anyhow!(
                    "unsupported desktop audio sample format: {sample_format:?}; DSD is not PCM"
                ));
            }
            other => {
                return Err(anyhow!(
                    "unsupported desktop audio sample format: {other:?}"
                ));
            }
        };
        stream
            .play()
            .context("starting desktop audio output stream")?;

        Ok(Self {
            _stream: stream,
            state,
            device_buffer_samples,
        })
    }

    pub(super) fn queue(&self, samples: &[f32]) -> Result<()> {
        let mut queue = self
            .state
            .samples
            .lock()
            .map_err(|_| anyhow!("desktop audio queue lock poisoned"))?;
        queue.extend(samples.iter().copied());
        self.state
            .queued_samples
            .fetch_add((samples.len() / AUDIO_CHANNELS) as u64, Ordering::Release);
        Ok(())
    }

    pub(super) fn clear(&self) {
        if let Ok(mut samples) = self.state.samples.lock() {
            samples.clear();
        }
        self.state.queued_samples.store(0, Ordering::Release);
    }

    pub(super) fn pause(&self) {
        self.state.playing.store(false, Ordering::Release);
    }

    pub(super) fn resume(&self) {
        self.state.playing.store(true, Ordering::Release);
    }

    pub(super) fn queued_samples(&self) -> u64 {
        self.state.queued_samples.load(Ordering::Acquire)
    }

    pub(super) fn device_buffer_samples(&self) -> u64 {
        self.device_buffer_samples
    }
}

fn build_audio_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<AudioState>,
    error_callback: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| write_audio_data(output, &state),
            error_callback,
            None,
        )
        .context("building desktop audio output stream")
}

fn write_audio_data<T>(output: &mut [T], state: &AudioState)
where
    T: Sample + FromSample<f32>,
{
    if !state.playing.load(Ordering::Acquire) {
        output.fill(T::from_sample(0.0));
        return;
    }

    let Ok(mut queue) = state.samples.lock() else {
        output.fill(T::from_sample(0.0));
        return;
    };
    let mut consumed = 0_u64;
    for sample in output {
        if let Some(value) = queue.pop_front() {
            *sample = T::from_sample(value);
            consumed += 1;
        } else {
            *sample = T::from_sample(0.0);
        }
    }
    if consumed > 0 {
        state
            .queued_samples
            .fetch_sub(consumed / AUDIO_CHANNELS as u64, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_callback_keeps_buffered_frames_for_prebuffering() {
        let state = AudioState {
            samples: Mutex::new(VecDeque::from([0.25, -0.25])),
            queued_samples: AtomicU64::new(1),
            playing: AtomicBool::new(false),
        };
        let mut output = [1.0_f32; AUDIO_CHANNELS];

        write_audio_data(&mut output, &state);

        assert_eq!(output, [0.0, 0.0]);
        assert_eq!(state.queued_samples.load(Ordering::Acquire), 1);
    }

    #[test]
    fn callback_reports_consumed_stereo_frames() {
        let state = AudioState {
            samples: Mutex::new(VecDeque::from([0.25, -0.25, 0.5, -0.5])),
            queued_samples: AtomicU64::new(2),
            playing: AtomicBool::new(true),
        };
        let mut output = [0.0_f32; AUDIO_CHANNELS * 2];

        write_audio_data(&mut output, &state);

        assert_eq!(output, [0.25, -0.25, 0.5, -0.5]);
        assert_eq!(state.queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn callback_converts_silence_to_unsigned_8_bit_pcm() {
        let state = AudioState {
            samples: Mutex::new(VecDeque::from([0.0, 0.0])),
            queued_samples: AtomicU64::new(1),
            playing: AtomicBool::new(true),
        };
        let mut output = [0_u8; AUDIO_CHANNELS];

        write_audio_data(&mut output, &state);

        assert_eq!(output, [128, 128]);
        assert_eq!(state.queued_samples.load(Ordering::Acquire), 0);
    }
}
