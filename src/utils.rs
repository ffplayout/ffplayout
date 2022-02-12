use serde::{Deserialize, Serialize};
use serde_yaml::{self};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub general: General,
    pub mail: Mail,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct General {
    pub helptext: String,
    pub stop_threshold: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Mail {
    pub helptext: String,
    pub subject: String,
    pub smtp_server: String,
    pub smtp_port: u32,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: String,
}

pub fn read_yaml() -> Config {
    let f = std::fs::File::open("ffplayout.yml").expect("Could not open file.");
    let config: Config = serde_yaml::from_reader(f).expect("Could not read values.");

    config
}
