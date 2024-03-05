use std::sync::Arc;

extern crate log;
extern crate simplelog;

use lazy_static::lazy_static;

pub mod filter;
pub mod macros;
pub mod utils;

use utils::advanced_config::AdvancedConfig;

lazy_static! {
    pub static ref ADVANCED_CONFIG: Arc<AdvancedConfig> = Arc::new(AdvancedConfig::new());
}
