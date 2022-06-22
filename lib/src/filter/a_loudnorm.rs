use crate::utils::PlayoutConfig;

/// Loudnorm Audio Filter
///
/// Add loudness normalization.
pub fn filter_node(config: &PlayoutConfig) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={}",
        config.processing.loud_i, config.processing.loud_tp, config.processing.loud_lra
    )
}
