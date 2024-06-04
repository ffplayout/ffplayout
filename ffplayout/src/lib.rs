use std::sync::{Arc, Mutex};

use clap::Parser;
use lazy_static::lazy_static;
use sysinfo::{Disks, Networks, System};

pub mod api;
pub mod db;
pub mod macros;
pub mod player;
pub mod sse;
pub mod utils;

use utils::advanced_config::AdvancedConfig;
use utils::args_parse::Args;

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
    pub static ref DISKS: Arc<Mutex<Disks>> =
        Arc::new(Mutex::new(Disks::new_with_refreshed_list()));
    pub static ref NETWORKS: Arc<Mutex<Networks>> =
        Arc::new(Mutex::new(Networks::new_with_refreshed_list()));
    pub static ref SYS: Arc<Mutex<System>> = Arc::new(Mutex::new(System::new_all()));
}
