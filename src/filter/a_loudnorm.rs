use crate::utils::GlobalConfig;

/// Loudnorm Audio Filter
///
/// Add loudness normalization.
pub fn filter_node(config: &GlobalConfig) -> String {
    format!(
        "loudnorm=I={}:TP={}:LRA={}",
        config.processing.loud_i, config.processing.loud_tp, config.processing.loud_lra
    )
}
