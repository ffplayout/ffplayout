use std::{
    collections::HashSet,
    env, io,
    process::exit,
    sync::{Arc, Mutex},
    thread,
};

use actix_files::Files;
use actix_web::{
    dev::ServiceRequest, middleware::Logger, web, App, Error, HttpMessage, HttpServer,
};
use actix_web_grants::authorities::AttachAuthorities;
use actix_web_httpauth::{extractors::bearer::BearerAuth, middleware::HttpAuthentication};

#[cfg(all(not(debug_assertions), feature = "embed_frontend"))]
use actix_web_static_files::ResourceFiles;

use log::*;
use path_clean::PathClean;

use ffplayout::{
    api::{auth, routes::*},
    db::{
        db_pool, handles,
        models::{init_globales, UserMeta},
    },
    player::controller::{ChannelController, ChannelManager},
    sse::{broadcast::Broadcaster, routes::*, AuthState},
    utils::{
        args_parse::run_args,
        config::PlayoutConfig,
        logging::{init_logging, MailQueue},
    },
    ARGS,
};

#[cfg(any(debug_assertions, not(feature = "embed_frontend")))]
use ffplayout::utils::public_path;

#[cfg(all(not(debug_assertions), feature = "embed_frontend"))]
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn thread_counter() -> usize {
    let available_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    (available_threads / 2).max(2)
}

async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    // We just get permissions from JWT
    match auth::decode_jwt(credentials.token()).await {
        Ok(claims) => {
            req.attach(vec![claims.role]);

            req.extensions_mut()
                .insert(UserMeta::new(claims.id, claims.channel));

            Ok(req)
        }
        Err(e) => Err((e, req)),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mail_queues = Arc::new(Mutex::new(vec![]));

    let pool = db_pool()
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    if let Err(e) = handles::db_migrate(&pool).await {
        panic!("{e}");
    };

    if let Err(c) = run_args(&pool).await {
        exit(c);
    }

    init_globales(&pool).await;
    init_logging(mail_queues.clone())?;

    let channel_controllers = Arc::new(Mutex::new(ChannelController::new()));

    if let Some(conn) = &ARGS.listen {
        let channels = handles::select_all_channels(&pool)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        for channel in channels.iter() {
            let config = PlayoutConfig::new(&pool, channel.id).await;
            let manager = ChannelManager::new(Some(pool.clone()), channel.clone(), config.clone());
            let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail)));

            channel_controllers
                .lock()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
                .add(manager.clone());

            if let Ok(mut mqs) = mail_queues.lock() {
                mqs.push(m_queue.clone());
            }

            if channel.active {
                manager.async_start().await;
            }
        }

        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();
        let controllers = web::Data::from(channel_controllers);
        let auth_state = web::Data::new(AuthState {
            uuids: tokio::sync::Mutex::new(HashSet::new()),
        });
        let broadcast_data = Broadcaster::create();
        let thread_count = thread_counter();

        info!("Running ffplayout API, listen on http://{conn}");

        // no 'allow origin' here, give it to the reverse proxy
        HttpServer::new(move || {
            let queues = mail_queues.clone();

            let auth = HttpAuthentication::bearer(validator);
            let db_pool = web::Data::new(pool.clone());
            // Customize logging format to get IP though proxies.
            let logger = Logger::new("%{r}a \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T")
                .exclude_regex(r"/_nuxt/*");

            let mut web_app = App::new()
                .app_data(db_pool)
                .app_data(web::Data::from(queues))
                .app_data(controllers.clone())
                .app_data(auth_state.clone())
                .app_data(web::Data::from(Arc::clone(&broadcast_data)))
                .wrap(logger)
                .service(login)
                .service(
                    web::scope("/api")
                        .wrap(auth.clone())
                        .service(add_user)
                        .service(get_user)
                        .service(get_by_name)
                        .service(get_users)
                        .service(remove_user)
                        .service(get_advanced_config)
                        .service(update_advanced_config)
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
                        .service(get_program)
                        .service(get_system_stat)
                        .service(generate_uuid),
                )
                .service(
                    web::scope("/data")
                        .service(validate_uuid)
                        .service(event_stream),
                )
                .service(get_file);

            if let Some(public) = &ARGS.public {
                // When public path is set as argument use this path for serving extra static files,
                // is useful for HLS stream etc.
                let absolute_path = if public.is_absolute() {
                    public.to_path_buf()
                } else {
                    env::current_dir().unwrap_or_default().join(public)
                }
                .clean();

                web_app = web_app.service(Files::new("/", absolute_path));
            } else {
                // When no public path is given as argument, use predefine keywords in path,
                // like /live; /preview; /public, or HLS extensions to recognize file should get from public folder
                web_app = web_app.service(get_public);
            }

            #[cfg(all(not(debug_assertions), feature = "embed_frontend"))]
            {
                // in release mode embed frontend
                let generated = generate();
                web_app =
                    web_app.service(ResourceFiles::new("/", generated).resolve_not_found_to_root());
            }

            #[cfg(any(debug_assertions, not(feature = "embed_frontend")))]
            {
                // in debug mode get frontend from path
                web_app = web_app.service(Files::new("/", public_path()).index_file("index.html"));
            }

            web_app
        })
        .bind((addr, port))?
        .workers(thread_count)
        .run()
        .await
    } else if let Some(channels) = &ARGS.channels {
        for (index, channel_id) in channels.iter().enumerate() {
            let channel = handles::select_channel(&pool, channel_id).await.unwrap();
            let config = PlayoutConfig::new(&pool, *channel_id).await;
            let manager = ChannelManager::new(Some(pool.clone()), channel.clone(), config.clone());
            let m_queue = Arc::new(Mutex::new(MailQueue::new(*channel_id, config.mail)));

            channel_controllers
                .lock()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
                .add(manager.clone());

            if let Ok(mut mqs) = mail_queues.lock() {
                mqs.push(m_queue.clone());
            }

            manager.foreground_start(index).await;
        }

        Ok(())
    } else {
        error!("Run ffplayout with parameters! Run ffplayout -h for more information.");

        Ok(())
    }
}
