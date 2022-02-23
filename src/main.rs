mod filter;
mod output;
mod utils;

use crate::output::desktop;
use crate::utils::{get_config, Messenger};

fn main() {
    let config = get_config();
    let msg = Messenger::new(&config);

    msg.debug("this is a debug");
    msg.info("this is a info");
    msg.warning("this is a warning");
    msg.error("this is a error");
    // println!("{:#?}", config);

    // folder::walk(&config.storage.path, config.storage.shuffle, &config.storage.extensions);

    // read_json(&config);

    // let args = arg_parse::get_args();

    // println!("{:#?}", args);
    // println!("{:#?}", args.config.is_some());
    // // println!("{:#?}", args.config.unwrap());
    // //println!("{:?}", config.general.stop_threshold);

    // println!("{:#?}", utils::get_sec());
    // println!("{:#?}", utils::get_timestamp());

    desktop::play(config).expect("Play on desktop failed!");
}
