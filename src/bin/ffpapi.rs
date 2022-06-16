use std::{path::Path, process::exit};

use actix_web::{dev::ServiceRequest, middleware, web, App, Error, HttpMessage, HttpServer};
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
        routes::{
            add_preset, add_user, get_playout_config, get_presets, get_settings, login,
            patch_settings, update_playout_config, update_preset, update_user,
        },
        utils::{db_path, init_config, run_args, Role},
    },
    utils::{init_logging, PlayoutConfig},
};

async fn validator(req: ServiceRequest, credentials: BearerAuth) -> Result<ServiceRequest, Error> {
    // We just get permissions from JWT
    let claims = auth::decode_jwt(credentials.token()).await?;
    req.attach(vec![Role::set_role(&claims.role)]);

    req.extensions_mut()
        .insert(LoginUser::new(claims.id, claims.username));

    Ok(req)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mut config = PlayoutConfig::new(None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    if let Err(c) = run_args(args.clone()).await {
        exit(c);
    }

    if let Some(conn) = args.listen {
        if let Ok(p) = db_path() {
            if !Path::new(&p).is_file() {
                error!("Database is not initialized! Init DB first and add admin user.");
                exit(1);
            }
        }
        init_config().await;
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();

        info!("running ffplayout API, listen on {conn}");

        // TODO: add allow origin (or give it to the proxy)
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            App::new()
                .wrap(middleware::Logger::default())
                .service(login)
                .service(
                    web::scope("/api")
                        .wrap(auth)
                        .service(add_user)
                        .service(get_playout_config)
                        .service(update_playout_config)
                        .service(add_preset)
                        .service(get_presets)
                        .service(update_preset)
                        .service(get_settings)
                        .service(patch_settings)
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
