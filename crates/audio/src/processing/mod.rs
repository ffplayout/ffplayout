//! Gain, loudness correction, compression and lookahead limiting.
mod control;
mod dynamics;
mod lookahead;
mod loudness;
pub mod volume;

pub use control::{AudioEffect, AudioEffectChain, AudioEffectsControl};
pub use dynamics::LiveDynamicsProcessor;
pub use lookahead::{LIVE_LATENCY, TRUE_PEAK_FUTURE_SAMPLES, lookahead_samples};
pub use loudness::{
    BufferedLoudnessAnalysis, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor, LiveLoudnessSettings,
};
