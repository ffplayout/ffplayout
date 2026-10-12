use std::{hint::black_box, time::Instant};

use ff_engine::{LiveDynamicsProcessor, LiveLoudnessConfig};
use ffmpeg_next::{
    ChannelLayout,
    format::{Sample, sample::Type},
    frame,
};

const RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = 1024;
const SECONDS: usize = 12;

fn main() {
    ffmpeg_next::init().unwrap();

    for scenario in ["quiet", "loud", "transitions"] {
        let source: Vec<[f32; 2]> = (0..RATE as usize * SECONDS)
            .map(|index| {
                let amplitude = match scenario {
                    "quiet" => 0.01,
                    "loud" => 0.8,
                    _ => match index / RATE as usize % 4 {
                        0 => 0.01,
                        1 => 0.8,
                        2 => 0.00001,
                        _ => 0.1,
                    },
                };
                let sample = (std::f64::consts::TAU * 997.0 * index as f64 / f64::from(RATE)).sin()
                    as f32
                    * amplitude;
                [sample, sample * 0.7]
            })
            .collect();
        let mut times = Vec::new();

        for _ in 0..5 {
            let mut processor =
                LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap();
            let preview = processor.lookahead_samples();
            let mut audio = frame::Audio::new(
                Sample::F32(Type::Planar),
                FRAME_SAMPLES,
                ChannelLayout::STEREO,
            );
            audio.set_rate(RATE);
            let started = Instant::now();

            for offset in (0..source.len() - FRAME_SAMPLES).step_by(FRAME_SAMPLES) {
                for channel in [0, 1] {
                    for (index, sample) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                        *sample = source[offset + index][channel];
                    }
                }

                let future_start = offset + FRAME_SAMPLES;
                processor.process(
                    &mut audio,
                    &source[future_start..(future_start + preview).min(source.len())],
                );
                black_box(audio.plane::<f32>(0));
            }

            times.push(started.elapsed().as_secs_f64());
            black_box(processor.metrics());
        }

        times.sort_by(f64::total_cmp);
        println!(
            "{scenario}: {:.2} ms / {SECONDS}s audio; {:.2}% of one CPU core (median of 5)",
            times[2] * 1000.0,
            times[2] / SECONDS as f64 * 100.0
        );
    }
}
