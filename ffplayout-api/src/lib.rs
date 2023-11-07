use clap::Parser;
use lazy_static::lazy_static;
use std::sync::Mutex;
use sysinfo::{System, SystemExt};

pub mod api;
pub mod db;
pub mod utils;

use utils::args_parse::Args;

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
    pub static ref SYS: Mutex<System> = Mutex::new(System::new_all());
}
