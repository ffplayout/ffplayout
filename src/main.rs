extern crate log;
extern crate simplelog;

mod filter;
mod input;
mod output;
mod utils;

use simplelog::*;
use tokio::runtime::Runtime;

use crate::output::play;
use crate::utils::{init_config, init_logging};

fn main() {
    init_config();

    let runtime = Runtime::new().unwrap();
    let rt_handle = runtime.handle();

    let logging = init_logging(rt_handle.clone());
    CombinedLogger::init(logging).unwrap();

    play(rt_handle);
}
