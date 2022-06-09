use std::{process::exit, sync::Mutex};

use actix_web::{
    dev::ServiceRequest,
    middleware,
    web::{self, Data},
    App, Error, HttpServer,
};
use actix_web_grants::permissions::AttachPermissions;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;

use clap::Parser;
use simplelog::*;

use ffplayout_engine::{
    api::{
        args_parse::Args,
        auth,
        models::LoginUser,
        routes::{login, settings, update_user},
        utils::{init_config, run_args},
    },
    utils::{init_logging, GlobalConfig},
};

async fn validator(req: ServiceRequest, credentials: BearerAuth) -> Result<ServiceRequest, Error> {
    // We just get permissions from JWT
    let claims = auth::decode_jwt(credentials.token()).await?;
    req.attach(claims.permissions);
    Ok(req)
}

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
        init_config().await;
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();
        let data = Data::new(Mutex::new(LoginUser { id: 0 }));

        info!("running ffplayout API, listen on {conn}");

        // TODO: add allow origin
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            App::new()
                .wrap(middleware::Logger::default())
                .app_data(Data::clone(&data))
                .service(login)
                .service(
                    web::scope("/api")
                        .wrap(auth)
                        .service(settings)
                        .service(update_user),
                )
        })
        .bind((addr, port))?
        .run()
        .await
    } else {
        error!("Run ffpapi with listen parameter!");

        Ok(())
    }
}
