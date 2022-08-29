use crate::utils::PlayoutConfig;

/// Overlay Filter
///
/// When a logo is set, we create here the filter for the server.
pub fn filter_node(config: &PlayoutConfig, add_tail: bool) -> String {
    let mut logo_chain = String::new();

    if !config.processing.add_logo {
        return logo_chain;
    }

    if let Some(fps) = config.processing.logo_fps.clone() {
        let opacity = format!(
            "format=rgba,colorchannelmixer=aa={}",
            config.processing.logo_opacity
        );
        let pts = format!("setpts=N/({fps}*TB)");
        logo_chain = format!(
            "null[v];movie={}:loop=0,{pts},{opacity}",
            config.processing.logo
        );

        if add_tail {
            logo_chain.push_str(
                format!("[l];[v][l]{}:shortest=1", config.processing.logo_filter).as_str(),
            );
        }
    };

    logo_chain
}
