mod arg_parse;
mod config;
mod utils;
// mod folder;
mod json;

fn main() {
    let config = config::get_config();
    // println!("{:#?}", config);

    // folder::walk(&config.storage.path, config.storage.shuffle, &config.storage.extensions);

    json::read(&config);

    // let args = arg_parse::get_args();

    // println!("{:#?}", args);
    // println!("{:#?}", args.config.is_some());
    // // println!("{:#?}", args.config.unwrap());
    // //println!("{:?}", config.general.stop_threshold);

    // println!("{:#?}", utils::get_sec());
    // println!("{:#?}", utils::get_timestamp());
}
