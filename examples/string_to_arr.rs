use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml::{self};

use shlex::split;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Processing {
    pub mode: String,
    pub volume: f64,
    pub settings: String,
}

fn main() {
    let s = r#"
mode: "playlist"
volume: 0.5
settings: -i input.mp4 -c:v libx264 -metadata service_provider='ffplayout Inc.' -f mpegts out.mp4
"#;
    let config: Processing =
    serde_yaml::from_str(s).expect("Could not read config");

    let pattern = Regex::new(r#"[^\s"']+|"([^"]*)"|'([^']*)'"#).unwrap();

    let matches: Vec<String> = pattern
        .find_iter(config.settings.as_str())
        .map(|m| m.as_str().to_string())
        .collect();

    println!("{:#?}", matches);
    println!("{:#?}", split(config.settings.as_str()));
}
