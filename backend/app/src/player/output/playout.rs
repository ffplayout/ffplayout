use std::{
    fmt,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};

use ff_engine::{AsyncPlayout, ClipResult, LogoConfig, OutputConfig};
use log::*;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    player::{controller::ChannelManager, input::source_generator, utils::sec_to_time},
    utils::{
        config::{OutputMode, PlayoutConfig},
        errors::ServiceError,
        logging::Target,
        task_runner,
    },
};

pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    validate_supported_config(&config)?;

    let output_config = engine_output_config(&config)?;
    let playout = open_playout(&config, output_config.clone()).await?;
    if config.output.mode == OutputMode::Desktop {
        info!(target: Target::file_mail(), channel = config.general.channel_id;
            "Desktop output uses backend/engine SDL2 renderer"
        );
    }

    if config.ingest.enable {
        let url = ingest_url(&config)?;
        playout
            .start_rtmp_live(url.clone(), output_config)
            .await
            .map_err(engine_error)?;
        info!(target: Target::file_mail(), channel = config.general.channel_id;
            "Start ingest server, listening on: <span class=\"log-addr\">{url}</span>"
        );
    }

    let result = match config.output.mode {
        OutputMode::HLS => play_hls(manager.clone(), &config, &playout).await,
        _ => play_loop(manager.clone(), &config, &playout, None).await,
    };

    let finish_result = playout.finish().await.map_err(engine_error);
    result?;
    finish_result
}

async fn play_hls(
    manager: ChannelManager,
    config: &PlayoutConfig,
    playout: &AsyncPlayout,
) -> Result<(), ServiceError> {
    let output_cmd = config.output.output_cmd.clone().unwrap_or_default();
    let hls_duration = output_cmd
        .windows(2)
        .find_map(|pair| {
            if pair[0] == "-hls_time" {
                pair[1].parse::<u64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(10);
    let m3u8_path = hls_playlist_path(config)?;
    let watchdog_token = CancellationToken::new();
    let mut watchdog = tokio::spawn(hls_watchdog(
        config.general.channel_id,
        m3u8_path,
        Duration::from_secs(hls_duration),
        manager.is_alive.clone(),
        watchdog_token.clone(),
    ));

    let result = tokio::select! {
        result = &mut watchdog => {
            result??;
            Ok(())
        }
        result = play_loop(manager, config, playout, Some(watchdog_token.clone())) => result,
    };

    watchdog_token.cancel();
    result
}

async fn hls_watchdog(
    channel_id: i32,
    m3u8_path: PathBuf,
    hls_time: Duration,
    is_alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancel_token: CancellationToken,
) -> Result<(), ServiceError> {
    let mut init = true;
    let timeout = hls_time * 3;

    loop {
        if cancel_token.is_cancelled() {
            break;
        }
        let sleep_time = if init {
            init = false;
            timeout
        } else {
            hls_time
        };

        tokio::select! {
            _ = cancel_token.cancelled() => break,
            _ = sleep(sleep_time) => {}
        }

        if cancel_token.is_cancelled() || !is_alive.load(Ordering::SeqCst) {
            break;
        }

        if let Ok(last_mod) = m3u8_path
            .metadata()
            .and_then(|metadata| metadata.modified())
        {
            let age = SystemTime::now()
                .duration_since(last_mod)
                .unwrap_or_default();

            if age > timeout {
                error!(target: Target::file_mail(), channel = channel_id;
                    "HLS segment write timeout! Last update: <span class=\"log-number\">{:.3}s</span>", age.as_secs_f32()
                );
                return Err(ServiceError::Conflict("Timeout".to_string()));
            }
        }
    }

    Ok(())
}

async fn play_loop(
    manager: ChannelManager,
    config: &PlayoutConfig,
    playout: &AsyncPlayout,
    cancel_token: Option<CancellationToken>,
) -> Result<(), ServiceError> {
    let id = config.general.channel_id;
    let mut node_sources = source_generator(manager.clone()).await;

    while let Some(node) = node_sources.next().await {
        *manager.current_media.lock().await = Some(node.clone());

        if cancel_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
            || !manager.is_alive.load(Ordering::SeqCst)
        {
            break;
        }

        if node.skip {
            continue;
        }

        validate_supported_node(config, &node.source, &node.audio)?;

        info!(target: Target::file_mail(), channel = id;
            "Play for <span class=\"log-number\">{}</span>: <span class=\"log-addr\">{}</span>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                tokio::spawn(task_runner::run(manager.clone()));
            } else {
                error!(target: Target::file_mail(), channel = id;
                    "<span class=\"log-cmd\">{:?}</span> executable not exists!",
                    config.task.path
                );
            }
        }

        let duration = (node.out > node.seek).then_some(node.out - node.seek);
        match playout
            .play_with_timing(
                node.source.clone(),
                (node.seek > 0.0).then_some(node.seek),
                duration,
            )
            .await
            .map_err(engine_error)?
        {
            ClipResult::Played => {}
            ClipResult::Fallback { reason } => {
                error!(target: Target::file_mail(), channel = id;
                    "failed while playing {}: {reason}; fallback generated",
                    node.source
                );
            }
            ClipResult::Stopped => {
                // Only the desktop output ever produces `Stopped` (the user
                // closed the player window or pressed Escape). Returning
                // `Ok(())` here alone would make the channel supervisor
                // treat this as "finished without error" and immediately
                // restart the channel/window. Explicitly deactivate the
                // channel and stop the whole process so closing the window
                // actually shuts ffplayout down.
                //
                // Note: this code runs *inside* the channel's own
                // supervisor task, so we must not call
                // `manager.abort_supervisor()` here - that would await the
                // task's own `JoinHandle`, deadlock, and self-abort before
                // `process::exit` is ever reached. `process::exit`
                // terminates the whole process unconditionally, so there is
                // no need to cancel the current (or any other) task first.
                info!(target: Target::file_mail(), channel = id;
                    "Desktop player window closed; shutting down ffplayout"
                );
                manager.channel.lock().await.active = false;
                manager.stop_all(false).await;
                std::process::exit(0);
            }
        }

        if let Some(token) = &cancel_token {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(Duration::from_millis(1)) => {}
            }
        }
    }

    Ok(())
}

async fn open_playout(
    config: &PlayoutConfig,
    output_config: OutputConfig,
) -> Result<AsyncPlayout, ServiceError> {
    let fallback_duration = config.storage.filler_path.metadata().map_or(10.0, |_| 10.0);

    match config.output.mode {
        OutputMode::HLS => AsyncPlayout::open_hls(
            hls_playlist_path(config)?.to_string_lossy().to_string(),
            output_config,
            fallback_duration,
            Vec::new(),
            config.processing.vtt_enable,
        )
        .await
        .map_err(engine_error),
        OutputMode::Null => AsyncPlayout::open_null(output_config, fallback_duration)
            .await
            .map_err(engine_error),
        OutputMode::Stream => {
            AsyncPlayout::open(output_url(config)?, output_config, fallback_duration)
                .await
                .map_err(engine_error)
        }
        OutputMode::Desktop => open_desktop_playout(output_config, fallback_duration).await,
    }
}

#[cfg(feature = "desktop")]
async fn open_desktop_playout(
    output_config: OutputConfig,
    fallback_duration: f64,
) -> Result<AsyncPlayout, ServiceError> {
    AsyncPlayout::open_desktop(output_config, fallback_duration)
        .await
        .map_err(engine_error)
}

#[cfg(not(feature = "desktop"))]
async fn open_desktop_playout(
    _output_config: OutputConfig,
    _fallback_duration: f64,
) -> Result<AsyncPlayout, ServiceError> {
    Err(ServiceError::Conflict(
        "desktop output is not available because ffplayout was built without the desktop feature"
            .to_string(),
    ))
}

fn engine_output_config(config: &PlayoutConfig) -> Result<OutputConfig, ServiceError> {
    let width = u32::try_from(config.processing.width)
        .map_err(|_| ServiceError::Conflict("processing width must be positive".to_string()))?;
    let height = u32::try_from(config.processing.height)
        .map_err(|_| ServiceError::Conflict("processing height must be positive".to_string()))?;
    if width == 0 || height == 0 {
        return Err(ServiceError::Conflict(
            "processing size must be greater than zero".to_string(),
        ));
    }
    if !config.processing.fps.is_finite() || config.processing.fps <= 0.0 {
        return Err(ServiceError::Conflict(
            "processing fps must be a positive number".to_string(),
        ));
    }
    let fps = config.processing.fps.round() as u32;

    let logo = config.processing.add_logo.then(|| LogoConfig {
        path: config.processing.logo_path.clone(),
        scale: (!config.processing.logo_scale.trim().is_empty())
            .then(|| config.processing.logo_scale.clone()),
        opacity: config.processing.logo_opacity,
        position: config.processing.logo_position.clone(),
    });

    Ok(OutputConfig::new(width, height, fps, 48_000)
        .with_volume(config.processing.volume)
        .with_logo(logo))
}

fn validate_supported_config(config: &PlayoutConfig) -> Result<(), ServiceError> {
    let processing = &config.processing;
    let unsupported = [
        (processing.audio_only, "audio_only"),
        (processing.copy_audio, "copy_audio"),
        (processing.copy_video, "copy_video"),
        (processing.override_filter, "override_filter"),
        (!processing.custom_filter.trim().is_empty(), "custom_filter"),
        (processing.audio_tracks != 1, "audio_tracks != 1"),
        (
            processing.audio_track_index > 0,
            "audio_track_index other than default/first track",
        ),
        (config.text.add_text, "text overlay"),
        (
            config.output.output_filter.is_some(),
            "output filter_complex",
        ),
        (
            config.advanced.decoder.input_cmd.is_some()
                || config.advanced.decoder.output_cmd.is_some()
                || config.advanced.encoder.input_cmd.is_some(),
            "advanced ffmpeg command overrides",
        ),
    ];

    let unsupported = unsupported
        .into_iter()
        .filter_map(|(enabled, name)| enabled.then_some(name))
        .collect::<Vec<_>>();

    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(ServiceError::Conflict(format!(
            "backend/engine integration does not support these ffmpeg-binary features yet: {}",
            unsupported.join(", ")
        )))
    }
}

fn validate_supported_node(
    config: &PlayoutConfig,
    source: &str,
    audio: &str,
) -> Result<(), ServiceError> {
    if !audio.is_empty() {
        return Err(ServiceError::Conflict(
            "backend/engine integration does not support separate audio files yet".to_string(),
        ));
    }
    if Path::new(source)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            crate::utils::config::IMAGE_FORMAT
                .iter()
                .any(|image_extension| image_extension.eq_ignore_ascii_case(extension))
        })
    {
        return Err(ServiceError::Conflict(
            "backend/engine integration does not support image loop sources yet".to_string(),
        ));
    }
    if config.processing.vtt_enable && !Path::new(source).with_extension("vtt").is_file() {
        warn!(target: Target::file_mail(), channel = config.general.channel_id;
            "WebVTT enabled, but no sidecar subtitle file found for <span class=\"log-addr\">{source}</span>"
        );
    }

    Ok(())
}

fn output_url(config: &PlayoutConfig) -> Result<String, ServiceError> {
    config
        .output
        .output_cmd
        .as_deref()
        .and_then(last_output_argument)
        .ok_or_else(|| {
            ServiceError::Conflict(
                "could not resolve stream output URL from output parameters".to_string(),
            )
        })
}

fn hls_playlist_path(config: &PlayoutConfig) -> Result<PathBuf, ServiceError> {
    if let Some(path) = config.output.output_cmd.as_deref().and_then(|cmd| {
        cmd.iter()
            .find(|item| item.ends_with(".m3u8") && !item.ends_with("master.m3u8"))
    }) {
        return Ok(PathBuf::from(path));
    }

    if config
        .channel
        .public
        .extension()
        .is_some_and(|extension| extension == "m3u8")
    {
        Ok(config.channel.public.clone())
    } else {
        Ok(config.channel.public.join("index.m3u8"))
    }
}

fn ingest_url(config: &PlayoutConfig) -> Result<String, ServiceError> {
    config
        .ingest
        .input_cmd
        .as_deref()
        .and_then(|cmd| cmd.iter().find(|item| item.contains("://")).cloned())
        .ok_or_else(|| ServiceError::Conflict("could not resolve ingest URL".to_string()))
}

fn last_output_argument(cmd: &[String]) -> Option<String> {
    cmd.iter()
        .enumerate()
        .rev()
        .find(|(index, item)| {
            *index > 0
                && !item.starts_with('-')
                && cmd
                    .get(index.saturating_sub(1))
                    .is_some_and(|previous| !previous.starts_with('-'))
        })
        .map(|(_, item)| item.clone())
}

fn engine_error(error: impl fmt::Display) -> ServiceError {
    ServiceError::Conflict(error.to_string())
}
