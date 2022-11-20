use std::{path::Path, process::exit};

use actix_files::Files;
use actix_web::{dev::ServiceRequest, middleware, web, App, Error, HttpMessage, HttpServer};
use actix_web_grants::permissions::AttachPermissions;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;

use clap::Parser;
use simplelog::*;

pub mod api;
pub mod db;
pub mod utils;

use api::{
    auth,
    routes::{
        add_channel, add_dir, add_preset, add_user, control_playout, del_playlist, delete_preset,
        file_browser, gen_playlist, get_all_channels, get_channel, get_log, get_playlist,
        get_playout_config, get_presets, get_program, get_user, import_playlist, login,
        media_current, media_last, media_next, move_rename, patch_channel, process_control, remove,
        remove_channel, save_file, save_playlist, send_text_message, update_playout_config,
        update_preset, update_user,
    },
};
use db::{db_pool, models::LoginUser};
use utils::{args_parse::Args, db_path, init_config, run_args, Role};

use ffplayout_lib::utils::{init_logging, PlayoutConfig};

async fn validator(req: ServiceRequest, credentials: BearerAuth) -> Result<ServiceRequest, Error> {
    // We just get permissions from JWT
    let claims = auth::decode_jwt(credentials.token()).await?;
    req.attach(vec![Role::set_role(&claims.role)]);

    req.extensions_mut()
        .insert(LoginUser::new(claims.id, claims.username));

    Ok(req)
}

fn public_path() -> &'static str {
    if Path::new("/usr/share/ffplayout/public/").is_dir() {
        return "/usr/share/ffplayout/public/";
    }

    if Path::new("./public/").is_dir() {
        return "./public/";
    }

    "./ffplayout-frontend/dist"
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

    let pool = match db_pool().await {
        Ok(p) => p,
        Err(e) => {
            error!("{e}");
            exit(1);
        }
    };

    if let Err(c) = run_args(&pool, args.clone()).await {
        exit(c);
    }

    if let Some(conn) = args.listen {
        if let Ok(p) = db_path() {
            if !Path::new(&p).is_file() {
                error!("Database is not initialized! Init DB first and add admin user.");
                exit(1);
            }
        }
        init_config(&pool).await;
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();

        info!("running ffplayout API, listen on {conn}");

        // no allow origin here, give it to the reverse proxy
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            let db_pool = web::Data::new(pool.clone());

            App::new()
                .app_data(db_pool)
                .wrap(middleware::Logger::default())
                .service(login)
                .service(
                    web::scope("/api")
                        .wrap(auth)
                        .service(add_user)
                        .service(get_user)
                        .service(get_playout_config)
                        .service(update_playout_config)
                        .service(add_preset)
                        .service(get_presets)
                        .service(update_preset)
                        .service(delete_preset)
                        .service(get_channel)
                        .service(get_all_channels)
                        .service(patch_channel)
                        .service(add_channel)
                        .service(remove_channel)
                        .service(update_user)
                        .service(send_text_message)
                        .service(control_playout)
                        .service(media_current)
                        .service(media_next)
                        .service(media_last)
                        .service(process_control)
                        .service(get_playlist)
                        .service(save_playlist)
                        .service(gen_playlist)
                        .service(del_playlist)
                        .service(get_log)
                        .service(file_browser)
                        .service(add_dir)
                        .service(move_rename)
                        .service(remove)
                        .service(save_file)
                        .service(import_playlist)
                        .service(get_program),
                )
                .service(Files::new("/", public_path()).index_file("index.html"))
        })
        .bind((addr, port))?
        .run()
        .await
    } else {
        error!("Run ffpapi with listen parameter!");

        Ok(())
    }
}
