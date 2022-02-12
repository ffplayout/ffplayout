mod utils;

fn main() {
    let config = utils::read_yaml();
    println!("{:#?}", config);
    //println!("{:?}", config.general.stop_threshold);
}
