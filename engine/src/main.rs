use std::{
    env,
    sync::{Arc, LazyLock, atomic::AtomicBool},
};

use axum::{Router, middleware};
use lazy_limit::{Duration as LDuration, RuleConfig, init_rate_limiter};
use log::*;
use protect_axum::GrantsLayer;
use real::RealIpLayer;
use tokio::{
    fs::File,
    io::AsyncReadExt,
    net::TcpListener,
    sync::{Mutex, RwLock},
};
use tokio_util::sync::CancellationToken;

use ffplayout::{
    ARGS,
    api::{self, state::AppState},
    db::{db_drop, db_pool, handles, init_globales},
    extract,
    middleware::governor::rate_limit,
    player::{
        controller::{ChannelController, ChannelManager},
        utils::{JsonPlaylist, get_date, is_remote, json_validate::validate_playlist},
    },
    sse::{SseAuthState, broadcast::Broadcaster},
    utils::{
        args_parse::init_args,
        config::get_config,
        errors::ProcessError,
        logging::{init_logging, log_middleware},
        mail::{self, MailQueue},
        playlist::generate_playlist,
        system::SystemStat,
        time_machine::set_mock_time,
    },
};

#[cfg(not(debug_assertions))]
use ffplayout::serve::routes::admin_ui_routes;

fn env_parse_or<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr,
{
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

pub static MAX_BLOCKING_THREADS: LazyLock<usize> =
    LazyLock::new(|| env_parse_or("MAX_BLOCKING_THREADS", 16));

fn main() -> Result<(), ProcessError> {
    #[cfg(feature = "tokio-console")]
    console_subscriber::init();

    tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(*MAX_BLOCKING_THREADS)
        .name("ff-tokio-runtime")
        .thread_name("ff-tokio-worker")
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<(), ProcessError> {
    let pool = db_pool().await?;

    let mut init = init_args(&pool).await?;

    if ARGS.init {
        return Ok(());
    }

    set_mock_time(&ARGS.fake_time)?;
    init_globales(&pool).await?;
    let system = SystemStat::new();

    let app_state = AppState {
        auth_state: Arc::new(SseAuthState::default()),
        broadcaster: Broadcaster::create(system.clone()),
        controller: Arc::new(RwLock::new(ChannelController::new())),
        mail_queues: Arc::new(Mutex::new(vec![])),
        pool: pool.clone(),
        system: system.clone(),
    };

    // Logger handle should be kept alive until the end.
    let _logger = init_logging(app_state.mail_queues.clone())?;

    if let Some(conn) = &ARGS.listen {
        let channels = handles::select_related_channels(&pool, None).await?;

        for channel in channels {
            let config = get_config(&pool, channel.id).await?;
            let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
            let channel_active = channel.active;
            let manager = ChannelManager::new(pool.clone(), channel, config, system.clone()).await;

            if init {
                if let Err(e) = manager.storage.copy_assets().await {
                    error!("{e}");
                }

                init = false;
            }

            app_state.mail_queues.lock().await.push(m_queue);

            if channel_active {
                manager.start().await?;
            }

            app_state.controller.write().await.add(manager);
        }

        init_rate_limiter!(
            default: RuleConfig::new(LDuration::seconds(1), 16), // 16 req/s globally
            max_memory: Some(64 * 1024 * 1024), // 64MB max memory
            routes: [
                ("/auth/", RuleConfig::new(LDuration::minutes(1), 3).match_prefix(true)), // 3 req/min
            ]
        )
        .await;

        let listener = TcpListener::bind(conn)
            .await
            .map_err(|e| ProcessError::Input(format!("Failed to bind {conn}: {e}")))?;

        let app = Router::new()
            .merge(api::path::routes())
            .with_state(app_state.clone())
            .layer(RealIpLayer::default())
            .layer(GrantsLayer::with_extractor(extract))
            .layer(middleware::from_fn(log_middleware))
            .layer(middleware::from_fn(rate_limit));

        #[cfg(not(debug_assertions))]
        let app = app.merge(admin_ui_routes());

        info!("Running ffplayout, listen on http://{conn}");

        axum::serve(listener, app)
            .await
            .map_err(|e| ProcessError::Custom(e.to_string()))?;
    } else if ARGS.drop_db {
        db_drop().await;
    } else if let Some(channel_ids) = &ARGS.channel {
        for (index, channel_id) in channel_ids.iter().enumerate() {
            let config = get_config(&pool, *channel_id).await?;
            let channel = handles::select_channel(&pool, channel_id).await?;
            let manager =
                ChannelManager::new(pool.clone(), channel, config.clone(), system.clone()).await;

            if ARGS.foreground {
                let m_queue = Arc::new(Mutex::new(MailQueue::new(*channel_id, config.mail)));

                app_state.controller.write().await.add(manager.clone());
                app_state.mail_queues.lock().await.push(m_queue);

                manager.foreground_start(index).await?;
            } else if ARGS.generate.is_some() {
                // Run a simple playlist generator and save it to disk.
                generate_playlist(manager).await?;
            } else if ARGS.validate {
                let mut playlist_path = config.channel.playlists.clone();
                let start_sec = config.playlist.start_sec.unwrap_or_default();
                let date = get_date(false, start_sec, false, &config.channel.timezone);

                if playlist_path.is_dir() || is_remote(&playlist_path.to_string_lossy()) {
                    let d: Vec<&str> = date.split('-').collect();
                    playlist_path = playlist_path
                        .join(d[0])
                        .join(d[1])
                        .join(date.clone())
                        .with_extension("json");
                }

                debug!("Read: <span class=\"log-addr\">{playlist_path:?}</span>");

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
                    Arc::new(AtomicBool::new(true)),
                    CancellationToken::new(),
                )
                .await;
            } else if ARGS.test_mail {
                mail::send_mail(
                    &config.mail,
                    "This is just a test email...".to_string(),
                    false,
                )
                .await?;
            }
        }
    } else {
        error!(
            "Run ffplayout with correct parameters! For example:\n    -l 127.0.0.1:8787\n    --channel 1 2 --foreground\n    --channel 1 --generate 2025-01-20 - 2025-01-25\nRun ffplayout -h for more information."
        );
    }

    let managers = app_state.controller.read().await.managers.clone();

    for manager in &managers {
        manager.channel.lock().await.active = false;
        manager.stop_all(false).await;
        manager.abort_supervisor().await;
    }

    pool.close().await;

    Ok(())
}
