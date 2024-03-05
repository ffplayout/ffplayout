use std::sync::{Arc, Mutex};

extern crate log;
extern crate simplelog;

use lazy_static::lazy_static;

pub mod filter;
pub mod macros;
pub mod utils;

use utils::advanced_config::AdvancedConfig;

lazy_static! {
    pub static ref ADVANCED_CONFIG: Arc<Mutex<AdvancedConfig>> =
        Arc::new(Mutex::new(AdvancedConfig::new()));
}
