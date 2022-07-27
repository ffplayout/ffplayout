use std::sync::{Arc, Mutex};

use crate::filter::{a_loudnorm, v_drawtext, v_overlay};
use crate::utils::PlayoutConfig;

/// Audio Filter
///
/// If needed we add audio filters to the server instance.
fn audio_filter(config: &PlayoutConfig) -> String {
    let mut audio_chain = ";[0:a]afade=in:st=0:d=0.5".to_string();

    if config.processing.loudnorm_ingest {
        audio_chain.push(',');
        audio_chain.push_str(&a_loudnorm::filter_node(config));
    }

    if config.processing.volume != 1.0 {
        audio_chain.push_str(format!(",volume={}", config.processing.volume).as_str());
    }

    audio_chain.push_str("[aout1]");

    audio_chain
}

/// Create filter nodes for ingest live stream.
pub fn filter_cmd(config: &PlayoutConfig, filter_chain: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    let mut filter = format!(
        "[0:v]fps={},scale={}:{},setdar=dar={},fade=in:st=0:d=0.5",
        config.processing.fps,
        config.processing.width,
        config.processing.height,
        config.processing.aspect
    );

    let overlay = v_overlay::filter_node(config, true);
    let drawtext = v_drawtext::filter_node(config, None, filter_chain);

    if !overlay.is_empty() {
        filter.push(',');
    }

    if !drawtext.is_empty() {
        filter.push(',');
    }

    filter.push_str(&overlay);
    filter.push_str(&drawtext);
    filter.push_str("[vout1]");
    filter.push_str(audio_filter(config).as_str());

    vec![
        "-filter_complex".to_string(),
        filter,
        "-map".to_string(),
        "[vout1]".to_string(),
        "-map".to_string(),
        "[aout1]".to_string(),
    ]
}
