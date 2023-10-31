use std::process::exit;

use actix_web::{
    dev::ServiceRequest, middleware::Logger, web, App, Error, HttpMessage, HttpServer,
};
use actix_web_grants::permissions::AttachPermissions;
use actix_web_httpauth::{extractors::bearer::BearerAuth, middleware::HttpAuthentication};
use actix_web_static_files::ResourceFiles;

use clap::Parser;
use simplelog::*;

pub mod api;
pub mod db;
pub mod utils;

use api::{auth, routes::*};
use db::{db_pool, models::LoginUser};
use utils::{args_parse::Args, control::ProcessControl, db_path, init_config, run_args, Role};

use ffplayout_lib::utils::{init_logging, PlayoutConfig};

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    // We just get permissions from JWT
    match auth::decode_jwt(credentials.token()).await {
        Ok(claims) => {
            req.attach(vec![Role::set_role(&claims.role)]);

            req.extensions_mut()
                .insert(LoginUser::new(claims.id, claims.username));

            Ok(req)
        }
        Err(e) => Err((e, req)),
    }
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

    let pool = match db_pool(&args.db).await {
        Ok(p) => p,
        Err(e) => {
            error!("{e}");
            exit(1);
        }
    };

    if let Some(conn) = args.listen {
        if db_path(args.db).is_err() {
            error!("Database is not initialized! Init DB first and add admin user.");
            exit(1);
        }
        init_config(&pool).await;
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();
        let engine_process = web::Data::new(ProcessControl::new());

        info!("running ffplayout API, listen on {conn}");

        // no allow origin here, give it to the reverse proxy
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            let db_pool = web::Data::new(pool.clone());
            let generated = generate();

            let logger = Logger::new("%{r}a \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T")
                .exclude_regex(r"/_nuxt/*");

            App::new()
                .app_data(db_pool)
                .app_data(engine_process.clone())
                .wrap(logger)
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
                .service(get_file)
                .service(ResourceFiles::new("/", generated))
        })
        .bind((addr, port))?
        .run()
        .await
    } else {
        error!("Run ffpapi with listen parameter!");

        Ok(())
    }
}
