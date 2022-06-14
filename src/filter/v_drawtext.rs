use std::path::Path;

use regex::Regex;

use crate::utils::{Media, PlayoutConfig};

pub fn filter_node(config: &PlayoutConfig, node: &mut Media) -> String {
    let mut filter = String::new();
    let mut font = String::new();

    if config.text.add_text {
        if Path::new(&config.text.fontfile).is_file() {
            font = format!(":fontfile='{}'", config.text.fontfile)
        }

        if config.text.over_pre && config.text.text_from_filename {
            let source = node.source.clone();
            let regex: Regex = Regex::new(&config.text.regex).unwrap();

            let text: String = match regex.captures(&source) {
                Some(t) => t[1].to_string(),
                None => source,
            };

            let escape = text
                .replace('\'', "'\\\\\\''")
                .replace('%', "\\\\\\%")
                .replace(':', "\\:");
            filter = format!("drawtext=text='{escape}':{}{font}", config.text.style)
        } else {
            filter = format!(
                "zmq=b=tcp\\\\://'{}',drawtext=text=''{font}",
                config.text.bind_address.replace(':', "\\:")
            )
        }
    }

    filter
}
