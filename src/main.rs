extern crate log;
extern crate simplelog;

mod filter;
mod output;
mod utils;

use simplelog::*;

use crate::output::desktop;
use crate::utils::{get_config, init_logging};

fn main() {
    let config = get_config();
    let logging = init_logging(&config);

    CombinedLogger::init(logging).unwrap();

    // warn!("this is a warning");
    // error!("this is a error");

    desktop::play(config);
}
