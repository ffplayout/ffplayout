use crate::utils::PlayoutConfig;

/// Overlay Filter
///
/// When a logo is set, we create here the filter for the server.
pub fn filter_node(config: &PlayoutConfig) -> String {
    let mut fps = config.processing.fps;
    let mut fps_filter = String::new();

    if !config.processing.add_logo {
        return String::new();
    }

    if let Some(f) = config.processing.logo_fps {
        fps = f;
    };

    if config.processing.fps != fps {
        fps_filter = format!(",fps={}", config.processing.fps);
    }

    format!(
        "null[v];movie={}:loop=0,setpts=N/({fps}*TB),format=rgba,colorchannelmixer=aa={}{fps_filter}[l];[v][l]{}:shortest=1",
        config.processing.logo, config.processing.logo_opacity, config.processing.logo_filter
    )
}
