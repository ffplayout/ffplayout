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

    warn!("this is a warning");
    error!("this is a error");

    // folder::walk(&config.storage.path, config.storage.shuffle, &config.storage.extensions);

    // read_json(&config);

    // let args = arg_parse::get_args();

    // println!("{:#?}", args);
    // println!("{:#?}", args.config.is_some());
    // // println!("{:#?}", args.config.unwrap());
    // //println!("{:?}", config.general.stop_threshold);

    // println!("{:#?}", utils::get_sec());
    // println!("{:#?}", utils::get_timestamp());

    desktop::play(config);
}
