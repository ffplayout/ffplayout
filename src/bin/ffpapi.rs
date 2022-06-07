use std::process::exit;

use actix_web::{App, HttpServer};
use clap::Parser;
use simplelog::*;

use ffplayout_engine::{
    api::{
        args_parse::Args,
        routes::{get_user, login},
        utils::run_args,
    },
    utils::{init_logging, GlobalConfig},
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mut config = GlobalConfig::new(None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    if let Err(c) = run_args(args.clone()).await {
        exit(c);
    }

    if let Some(conn) = args.listen {
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();
        info!("running ffplayout API, listen on {conn}");

        HttpServer::new(|| App::new().service(get_user).service(login))
            .bind((addr, port))?
            .run()
            .await
    } else {
        error!("Run ffpapi with listen parameter!");

        Ok(())
    }
}
