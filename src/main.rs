extern crate log;
extern crate simplelog;

mod filter;
mod output;
mod utils;

use simplelog::*;

use crate::output::play;
use crate::utils::{init_config, init_logging};

fn main() {
    init_config();
    let logging = init_logging();
    CombinedLogger::init(logging).unwrap();

    play();
}
