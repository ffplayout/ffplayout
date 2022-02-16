mod output;
mod utils;

use crate::output::desktop;
use crate::utils::get_config;

fn main() {
    let config = get_config();
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

    desktop::play(config.processing.settings).expect("Play on desktop failed!");
}
