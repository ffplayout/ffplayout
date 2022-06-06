use std::process::exit;

use actix_web::{App, HttpServer};
use clap::Parser;
use sha_crypt::{sha512_simple, Sha512Params};
use simplelog::*;

use ffplayout_engine::{
    api::{
        handles::{add_user, db_connection, db_init},
        routes::user,
    },
    utils::{init_logging, GlobalConfig},
};

#[derive(Parser, Debug)]
#[clap(version,
    name = "ffpapi",
    version = "0.1.0",
    about = "ffplayout REST API",
    long_about = None)]
pub struct Args {
    #[clap(short, long, help = "Listen on IP:PORT, like: 127.0.0.1:8080")]
    pub listen: Option<String>,

    #[clap(short, long, help = "Initialize Database")]
    pub init: bool,

    #[clap(short, long, help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help = "Admin email")]
    pub email: Option<String>,

    #[clap(short, long, help = "Admin password")]
    pub password: Option<String>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    if !args.init && args.listen.is_none() && args.username.is_none() {
        error!("Wrong number of arguments! Run ffpapi --help for more information.");

        exit(1);
    }

    let mut config = GlobalConfig::new(None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    if args.init {
        if let Err(e) = db_init().await {
            panic!("{e}");
        };

        exit(0);
    }

    if let Some(username) = args.username {
        if args.email.is_none() || args.password.is_none() {
            error!("Email/password missing!");
            exit(1);
        }

        let params = Sha512Params::new(10_000).expect("RandomError!");

        let hashed_password =
            sha512_simple(&args.password.unwrap(), &params).expect("Should not fail");

        match db_connection().await {
            Ok(pool) => {
                if let Err(e) =
                    add_user(&pool, &args.email.unwrap(), &username, &hashed_password, &1).await
                {
                    pool.close().await;
                    error!("{e}");
                    exit(1);
                };

                pool.close().await;
                info!("Create admin user \"{username}\" done...");

                exit(0);
            }
            Err(e) => {
                panic!("{e}")
            }
        }
    }

    if let Some(conn) = args.listen {
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();
        info!("running ffplayout API, listen on {conn}");

        HttpServer::new(|| App::new().service(user))
            .bind((addr, port))?
            .run()
            .await
    } else {
        panic!("Run ffpapi with listen parameter!")
    }
}
