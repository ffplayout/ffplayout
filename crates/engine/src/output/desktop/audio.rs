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
    #[cfg(not(test))]
    _stream: cpal::Stream,
    #[cfg(test)]
    _stream: Option<cpal::Stream>,
    state: Arc<AudioState>,
    device_buffer_samples: u64,
}

struct AudioState {
    samples: Mutex<VecDeque<f32>>,
    queued_samples: AtomicU64,
    playing: AtomicBool,
    xruns: AtomicU64,
    missing_samples: AtomicU64,
}

impl DesktopAudio {
    /// Exercise the real sample queue without opening an OS audio device.
    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        Self {
            _stream: None,
            state: Arc::new(AudioState {
                samples: Mutex::new(VecDeque::new()),
                queued_samples: AtomicU64::new(0),
                playing: AtomicBool::new(false),
                xruns: AtomicU64::new(0),
                missing_samples: AtomicU64::new(0),
            }),
            device_buffer_samples: AUDIO_DEVICE_BUFFER_SAMPLES,
        }
    }

    #[cfg(test)]
    pub(super) fn samples_for_test(&self) -> Vec<f32> {
        self.state.samples.lock().unwrap().iter().copied().collect()
    }

    pub(super) fn open(sample_rate: u32, channel_id: i32) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default audio output device"))?;
        let supported = device
            .supported_output_configs()
            .context("querying supported audio output configurations")?
            .filter(|config| {
                config.channels() as usize == AUDIO_CHANNELS
                    && config.min_sample_rate() <= sample_rate
                    && config.max_sample_rate() >= sample_rate
            })
            .max_by_key(|config| sample_format_quality(config.sample_format()))
            .ok_or_else(|| {
                anyhow!("no stereo audio output configuration supports {sample_rate} Hz")
            })?
            .with_sample_rate(sample_rate);
        let sample_format = supported.sample_format();
        let buffer_size = preferred_buffer_size(supported.buffer_size());
        let mut config: cpal::StreamConfig = supported.into();
        config.buffer_size = buffer_size;
        let state = Arc::new(AudioState {
            samples: Mutex::new(VecDeque::with_capacity(
                (sample_rate as usize * AUDIO_MAX_QUEUE_MS as usize / 1_000) * AUDIO_CHANNELS,
            )),
            queued_samples: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            xruns: AtomicU64::new(0),
            missing_samples: AtomicU64::new(0),
        });
        let callback_state = Arc::clone(&state);
        let error_state = Arc::clone(&state);
        let error_callback = move |error: cpal::Error| {
            // Logging can acquire locks and perform I/O. Never let an xrun warning
            // delay ALSA's recovery on the audio thread and cause another xrun.
            if error.kind() == cpal::ErrorKind::Xrun {
                error_state.xruns.fetch_add(1, Ordering::Relaxed);
            } else {
                log::warn!(channel = channel_id; "desktop audio stream error: {error}");
            }
        };
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
        let device_buffer_samples = stream
            .buffer_size()
            .map(u64::from)
            .unwrap_or_else(|_| match config.buffer_size {
                cpal::BufferSize::Fixed(size) => u64::from(size),
                cpal::BufferSize::Default => AUDIO_DEVICE_BUFFER_SAMPLES,
            })
            .max(1);
        log::debug!(channel = channel_id;
            "desktop audio device buffer: {device_buffer_samples} samples ({:.2} ms)",
            device_buffer_samples as f64 * 1000.0 / f64::from(sample_rate));
        stream
            .play()
            .context("starting desktop audio output stream")?;

        Ok(Self {
            #[cfg(not(test))]
            _stream: stream,
            #[cfg(test)]
            _stream: Some(stream),
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
            self.state.queued_samples.store(0, Ordering::Release);
        }
    }

    pub(super) fn take_underruns(&self) -> (u64, u64) {
        (
            self.state.xruns.swap(0, Ordering::Relaxed),
            self.state.missing_samples.swap(0, Ordering::Relaxed),
        )
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

/// Avoid tiny host-default periods: a broadcast preview does not need the
/// lowest possible device latency, and 1024 frames tolerate scheduler jitter.
fn preferred_buffer_size(supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } if min > 0 && min <= max => {
            cpal::BufferSize::Fixed((AUDIO_DEVICE_BUFFER_SAMPLES as u32).clamp(min, max))
        }
        _ => cpal::BufferSize::Default,
    }
}

/// CPAL does not guarantee that `supported_output_configs()` is ordered by
/// fidelity. Prefer floating-point PCM, then the widest integer formats, so a
/// device's legacy 8-bit configuration is never chosen merely because it was
/// listed first.
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
    let requested = output.len() as u64 / AUDIO_CHANNELS as u64;
    let mut consumed = 0_u64;

    for sample in output {
        if let Some(value) = queue.pop_front() {
            *sample = T::from_sample(value);
            consumed += 1;
        } else {
            *sample = T::from_sample(0.0);
        }
    }

    let missing = requested.saturating_sub(consumed / AUDIO_CHANNELS as u64);

    if missing > 0 {
        state.missing_samples.fetch_add(missing, Ordering::Relaxed);
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
    fn device_buffer_request_respects_supported_limits() {
        use cpal::{BufferSize, SupportedBufferSize};

        for (min, max, expected) in [(64, 8192, 1024), (2048, 8192, 2048), (64, 512, 512)] {
            assert_eq!(
                preferred_buffer_size(&SupportedBufferSize::Range { min, max }),
                BufferSize::Fixed(expected)
            );
        }

        assert_eq!(
            preferred_buffer_size(&SupportedBufferSize::Unknown),
            BufferSize::Default
        );
    }

    #[test]
    fn queue_underruns_count_only_missing_frames_and_keep_the_sample_clock() {
        let audio = DesktopAudio::for_test();
        audio.queue(&[0.25, -0.25]).unwrap();
        audio.resume();
        let mut output = [1.0_f32; AUDIO_CHANNELS * 3];
        write_audio_data(&mut output, &audio.state);
        assert_eq!(output, [0.25, -0.25, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(audio.queued_samples(), 0);
        assert_eq!(audio.take_underruns(), (0, 2));
        assert_eq!(audio.take_underruns(), (0, 0));
        audio.pause();
        write_audio_data(&mut output, &audio.state);
        assert_eq!(audio.take_underruns(), (0, 0));
    }

    #[test]
    fn clearing_queue_resets_samples_and_count_together() {
        let audio = DesktopAudio::for_test();
        audio.queue(&[0.25; 16]).unwrap();
        audio.clear();
        assert_eq!(audio.queued_samples(), 0);
        assert!(audio.samples_for_test().is_empty());
        audio.queue(&[0.5; 4]).unwrap();
        audio.resume();
        let mut output = [0.0_f32; 4];
        write_audio_data(&mut output, &audio.state);
        assert_eq!(output, [0.5; 4]);
        assert_eq!(audio.queued_samples(), 0);
    }

    #[test]
    fn prefers_high_fidelity_pcm_formats() {
        assert!(
            sample_format_quality(SampleFormat::F32) > sample_format_quality(SampleFormat::I16)
        );
        assert!(sample_format_quality(SampleFormat::I16) > sample_format_quality(SampleFormat::U8));
    }

    #[test]
    fn paused_callback_keeps_buffered_frames_for_prebuffering() {
        let state = AudioState {
            samples: Mutex::new(VecDeque::from([0.25, -0.25])),
            queued_samples: AtomicU64::new(1),
            playing: AtomicBool::new(false),
            xruns: AtomicU64::new(0),
            missing_samples: AtomicU64::new(0),
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
            xruns: AtomicU64::new(0),
            missing_samples: AtomicU64::new(0),
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
            xruns: AtomicU64::new(0),
            missing_samples: AtomicU64::new(0),
        };
        let mut output = [0_u8; AUDIO_CHANNELS];

        write_audio_data(&mut output, &state);

        assert_eq!(output, [128, 128]);
        assert_eq!(state.queued_samples.load(Ordering::Acquire), 0);
    }
}
