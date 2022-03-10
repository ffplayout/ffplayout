extern crate log;
extern crate simplelog;

mod filter;
mod output;
mod utils;

use simplelog::*;

use crate::output::play;
use crate::utils::{get_config, init_logging};

fn main() {
    let config = get_config();
    let logging = init_logging(&config);

    CombinedLogger::init(logging).unwrap();

    play(config);
}
