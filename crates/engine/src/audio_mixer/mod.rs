pub mod control_audio;
mod live_dynamics;
pub mod live_loudness;
mod lookahead;
pub(crate) mod program;
pub(crate) mod samples;
pub mod volume;

pub use control_audio::*;
pub use live_dynamics::LiveDynamicsProcessor;
pub use live_loudness::{
    BufferedLoudnessAnalysis, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor,
};

pub(crate) use ff_audio::{LIVE_LATENCY, TRUE_PEAK_FUTURE_SAMPLES, lookahead_samples};
pub(crate) use lookahead::collect_audio_preview;

/// Source selection is fixed for the lifetime of a playout instance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LoudnessScope {
    #[default]
    Off,
    Live,
    All,
}
