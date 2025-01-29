use std::{
    collections::HashSet,
    sync::{atomic::AtomicBool, Arc},
    thread,
};

use actix_web::{middleware::Logger, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;

#[cfg(any(debug_assertions, not(feature = "embed_frontend")))]
use actix_files::Files;

#[cfg(all(not(debug_assertions), feature = "embed_frontend"))]
use actix_web_static_files::ResourceFiles;

use log::*;
use tokio::{fs::File, io::AsyncReadExt, sync::Mutex};

use ffplayout::{
    api::routes::*,
    db::{db_drop, db_pool, handles, init_globales},
    player::{
        controller::{ChannelController, ChannelManager},
        utils::{get_date, is_remote, json_validate::validate_playlist, JsonPlaylist},
    },
    sse::{broadcast::Broadcaster, routes::*, SseAuthState},
    utils::{
        args_parse::run_args,
        config::get_config,
        errors::ProcessError,
        logging::init_logging,
        mail::{self, MailQueue},
        playlist::generate_playlist,
        time_machine::set_mock_time,
    },
    validator, ARGS,
};

#[cfg(any(debug_assertions, not(feature = "embed_frontend")))]
use ffplayout::utils::public_path;

#[cfg(all(not(debug_assertions), feature = "embed_frontend"))]
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn thread_counter() -> usize {
    let available_threads = thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1);

    (available_threads / 2).max(2)
}

#[tokio::main]
async fn main() -> Result<(), ProcessError> {
    let mail_queues = Arc::new(Mutex::new(vec![]));
    let pool = db_pool().await?;

    run_args(&pool).await?;

    if ARGS.init {
        return Ok(());
    }

    set_mock_time(&ARGS.fake_time)?;
    init_globales(&pool).await?;

    // LoggerHandle should be kept alive until the end
    let _logger = init_logging(mail_queues.clone());

    let channel_controllers = Arc::new(Mutex::new(ChannelController::new()));

    if let Some(conn) = &ARGS.listen {
        let channels = handles::select_related_channels(&pool, None).await?;

        for channel in channels.into_iter() {
            let config = get_config(&pool, channel.id).await?;
            let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
            let channel_active = channel.active;
            let manager = ChannelManager::new(pool.clone(), channel, config);

            mail_queues.lock().await.push(m_queue);

            if channel_active {
                manager.start().await?;
            }

            channel_controllers.lock().await.add(manager);
        }

        let (addr, port) = conn
            .split_once(':')
            .map(|(a, p)| (a, p.parse::<u16>().ok()))
            .and_then(|(a, p)| p.map(|p| (a, p)))
            .ok_or(ProcessError::IO(
                "<ADRESSE>:<PORT> needed! For example: 127.0.0.1:8787".to_string(),
            ))?;
        let controllers = web::Data::from(channel_controllers.clone());
        let queues = web::Data::from(mail_queues);
        let auth_state = web::Data::new(SseAuthState {
            uuids: Mutex::new(HashSet::new()),
        });
        let broadcast_data = Broadcaster::create();

        info!("Running ffplayout, listen on http://{conn}");

        let db_clone = pool.clone();

        // no 'allow origin' here, give it to the reverse proxy
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            let db_pool = web::Data::new(db_clone.clone());
            // Customize logging format to get IP though proxies.
            let logger = Logger::new("%{r}a \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T")
                .exclude_regex(r"/_nuxt/*");

            let mut web_app = App::new()
                .app_data(db_pool)
                .app_data(queues.clone())
                .app_data(controllers.clone())
                .app_data(auth_state.clone())
                .app_data(web::Data::from(Arc::clone(&broadcast_data)))
                .wrap(logger)
                .service(web::scope("/auth").service(login).service(refresh))
                .service(
                    web::scope("/api")
                        .wrap(auth)
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
                .service(get_file)
                .service(get_public);

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
        .workers(thread_counter())
        .run()
        .await?;
    } else if ARGS.drop_db {
        db_drop().await;
    } else if let Some(channel_ids) = &ARGS.channel {
        for (index, channel_id) in channel_ids.iter().enumerate() {
            let config = get_config(&pool, *channel_id).await?;
            let channel = handles::select_channel(&pool, channel_id).await?;
            let manager = ChannelManager::new(pool.clone(), channel, config.clone());

            if ARGS.foreground {
                let m_queue = Arc::new(Mutex::new(MailQueue::new(*channel_id, config.mail)));

                channel_controllers.lock().await.add(manager.clone());
                mail_queues.lock().await.push(m_queue);

                manager.foreground_start(index).await?;
            } else if ARGS.generate.is_some() {
                // run a simple playlist generator and save them to disk
                generate_playlist(manager).await?;
            } else if ARGS.validate {
                let mut playlist_path = config.channel.playlists.clone();
                let start_sec = config.playlist.start_sec.unwrap();
                let date = get_date(false, start_sec, false, &config.channel.timezone);

                if playlist_path.is_dir() || is_remote(&playlist_path.to_string_lossy()) {
                    let d: Vec<&str> = date.split('-').collect();
                    playlist_path = playlist_path
                        .join(d[0])
                        .join(d[1])
                        .join(date.clone())
                        .with_extension("json");
                }

                let mut f = File::options()
                    .read(true)
                    .write(false)
                    .open(&playlist_path)
                    .await?;

                let mut contents = String::new();
                f.read_to_string(&mut contents).await?;

                let playlist: JsonPlaylist = serde_json::from_str(&contents)?;

                validate_playlist(
                    config,
                    Arc::new(Mutex::new(Vec::new())),
                    playlist,
                    Arc::new(AtomicBool::new(false)),
                )
                .await;
            } else if ARGS.test_mail {
                mail::send_mail(&config.mail, "This is just a test email...".to_string()).await?;
            }
        }
    } else {
        error!(
            "Run ffplayout with correct parameters! For example:
            -l 127.0.0.1
            --channel 1 2 --foreground
            --channel 1 --generate 2025-01-20 - 2025-01-25
        Run ffplayout -h for more information."
        );
    }

    for manager in &channel_controllers.lock().await.managers {
        manager.channel.lock().await.active = false;
        manager.stop_all(false).await;
    }

    pool.close().await;

    Ok(())
}
