//! Real loudness analysis, compression and limiting, including concurrent channels.
use std::hint::black_box;

use anyhow::Result;
use clap::Parser;
use ff_engine::{LiveDynamicsProcessor, LiveLoudnessConfig, LiveLoudnessProcessor};
use ffmpeg_next::{
    ChannelLayout,
    format::{Sample, sample::Type},
    frame,
};
use rayon::{ThreadPoolBuilder, prelude::*};

#[path = "../../../benches/support/mod.rs"]
mod support;

const RATE: u32 = 48_000;
const SAMPLES: usize = 1024;
const FRAMES: usize = 48;
const CASES: &[&str] = &[
    "loudness_single",
    "dynamics_single",
    "dynamics_channels_serial",
    "dynamics_channels_parallel",
    "dynamics_channels_parallel_cold",
];

struct Channel {
    dynamics: LiveDynamicsProcessor,
    loudness: LiveLoudnessProcessor,
    audio: frame::Audio,
    input: Vec<[f32; 2]>,
}

impl Channel {
    fn new() -> Result<Self> {
        let config = LiveLoudnessConfig::default();
        let dynamics = LiveDynamicsProcessor::new(RATE, config)?;
        let loudness = LiveLoudnessProcessor::new(RATE, config)?;
        let mut audio =
            frame::Audio::new(Sample::F32(Type::Planar), SAMPLES, ChannelLayout::STEREO);

        audio.set_rate(RATE);

        let input = (0..FRAMES * SAMPLES
            + dynamics
                .lookahead_samples()
                .max(loudness.peak_lookahead_samples()))
            .map(|index| {
                let amplitude = [0.01, 0.8, 0.00001, 0.1][index / (SAMPLES * 12) % 4];
                let sample = (std::f64::consts::TAU * 997.0 * index as f64 / f64::from(RATE)).sin()
                    as f32
                    * amplitude;

                [sample, sample * 0.7]
            })
            .collect();

        Ok(Self {
            dynamics,
            loudness,
            audio,
            input,
        })
    }

    fn process(&mut self, dynamics: bool, cold: bool) -> Result<u64> {
        if cold {
            self.dynamics = LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default())?;
        }

        let preview = if dynamics {
            self.dynamics.lookahead_samples()
        } else {
            self.loudness.peak_lookahead_samples()
        };
        let mut checksum = 0;

        for frame_index in 0..FRAMES {
            let start = frame_index * SAMPLES;

            for channel in 0..2 {
                for (index, sample) in self.audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    *sample = self.input[start + index][channel];
                }
            }

            let end = start + SAMPLES;
            let future = &self.input[end..end + preview];

            if dynamics {
                self.dynamics.process(black_box(&mut self.audio), future);
            } else {
                self.loudness
                    .process_with_lookahead(black_box(&mut self.audio), future);
            }

            checksum ^= u64::from(black_box(self.audio.plane::<f32>(0)[512]).to_bits());
        }

        Ok(checksum)
    }
}

fn main() -> Result<()> {
    let args = support::Args::parse();

    let Some(case) = args.selected(CASES)? else {
        return Ok(());
    };

    ffmpeg_next::init()?;

    let count = if case.contains("channels") {
        args.threads
    } else {
        1
    };
    let mut channels = (0..count)
        .map(|_| Channel::new())
        .collect::<Result<Vec<_>>>()?;
    let pool = ThreadPoolBuilder::new().num_threads(args.threads).build()?;

    support::measure(&args, &case, FRAMES * SAMPLES * count, || {
        if case.contains("parallel") {
            pool.install(|| {
                channels
                    .par_iter_mut()
                    .map(|channel| channel.process(true, case.ends_with("cold")))
                    .sum::<Result<u64>>()
            })
        } else {
            channels
                .iter_mut()
                .map(|channel| channel.process(!case.starts_with("loudness"), false))
                .sum::<Result<u64>>()
        }
    })
}
