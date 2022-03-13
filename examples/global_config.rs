use serde::{Deserialize, Serialize};
use serde_yaml::{self};
use std::{fs::File};

use once_cell::sync::OnceCell;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub general: General,
    pub mail: Mail,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct General {
    pub stop_threshold: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Mail {
    pub subject: String,
    pub smtp_server: String,
    pub starttls: bool,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: String,
}

static INSTANCE: OnceCell<Config> = OnceCell::new();

impl Config {
    fn new() -> Self {
        let config_path = "/etc/ffplayout/ffplayout.yml".to_string();
        let f = File::open(&config_path).unwrap();

        let config: Config = serde_yaml::from_reader(f).expect("Could not read config file.");

        config
    }

    pub fn init() -> &'static Config {
        INSTANCE.get().expect("Config is not initialized")
    }
}

pub fn main() {
    let config = Config::new();
    INSTANCE.set(config).unwrap();
    let config = Config::init();

    println!("{:#?}", config);
}
