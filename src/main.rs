mod arg_parse;
mod config_reader;
mod utils;

fn main() {
    //println!("{:#?}", utils::Mail());
    let config = config_reader::read_yaml();
    let args = arg_parse::get_args();
    println!("{:#?}", config);
    println!("{:#?}", args);
    println!("{:#?}", args.config.is_some());
    println!("{:#?}", args.config.unwrap());
    //println!("{:?}", config.general.stop_threshold);

    println!("{:#?}", utils::get_sec());
    println!("{:#?}", utils::get_timestamp());
}
