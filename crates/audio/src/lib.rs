//! PCM signal processing, independent of codecs, devices and playout scheduling.
//! Each audio track owns its processors. Callers provide samples at the configured
//! sample rate and, where requested, contiguous future samples for lookahead.
//! Processing preserves sample counts; the caller schedules matching A/V latency.
pub use ebur128_stream::Error as AnalysisError;

pub mod analysis;
pub mod buffer;
pub mod processing;

#[cfg(test)]
mod test_support;

pub use analysis::{
    AudioLevel, AudioLevelCallback, AudioLevelMeter, LoudnessAnalyzer, LoudnessMeter,
    LoudnessMeterControl, LoudnessMetrics, level as meter, loudness,
};
pub use processing::{
    AudioEffect, AudioEffectChain, AudioEffectsControl, BufferedLoudnessAnalysis, LIVE_LATENCY,
    LiveDynamicsProcessor, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor, LiveLoudnessSettings, TRUE_PEAK_FUTURE_SAMPLES,
    lookahead_samples, volume,
};
