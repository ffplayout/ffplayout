pub mod control;
mod live_dynamics;
pub mod live_loudness;
mod lookahead;
pub(crate) mod program;
pub mod volume;

pub use control::*;
pub use live_dynamics::LiveDynamicsProcessor;
pub use live_loudness::{
    BufferedLoudnessAnalysis, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor,
};

pub(crate) use lookahead::{
    LIVE_LATENCY, TRUE_PEAK_FUTURE_SAMPLES, collect_audio_preview, lookahead_samples,
};

/// Source selection is fixed for the lifetime of a playout instance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LoudnessScope {
    #[default]
    Off,
    Live,
    All,
}
