use std::{
    fmt,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};

use ff_engine::{
    AsyncPlayout, ClipResult, LogoConfig, OutputConfig, RateControl, resolved_variant_playlist_path,
};
use log::*;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    file::norm_abs_path,
    player::{
        controller::ChannelManager,
        input::source_generator,
        utils::{Media, sec_to_time},
    },
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
        let url = config.ingest.ingest_url.clone();
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
    let hls_duration = u64::from(config.output.hls_segment_duration);
    let m3u8_path = watchdog_playlist_path(config)?;
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

        let duration = playout_duration(&node);
        match playout
            .play_with_timing(
                node.source.clone(),
                (node.seek > 0.0).then_some(node.seek),
                duration,
                subtitle_media_path(config, &node.source),
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
        OutputMode::HLS => {
            let hls_variants = config
                .output
                .parsed_hls_variants()
                .map_err(ServiceError::Conflict)?;

            AsyncPlayout::open_hls(
                hls_playlist_path(config)?.to_string_lossy().to_string(),
                output_config,
                fallback_duration,
                hls_variants,
                config.processing.vtt_enable,
                config.output.hls_segment_duration,
                config.output.hls_list_size,
            )
            .await
            .map_err(engine_error)
        }
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

    let rate_control = if config.output.rate_control == "crf" {
        RateControl::Crf
    } else {
        RateControl::Cbr
    };
    Ok(OutputConfig::new(width, height, fps, 48_000)
        .with_volume(config.processing.volume)
        .with_logo(logo)
        .with_encoding(
            config.output.video_preset.clone(),
            rate_control,
            config.output.video_quality,
            u64::from(config.output.video_maxrate) * 1_000,
            u64::from(config.output.audio_bitrate) * 1_000,
        ))
}

fn validate_supported_config(config: &PlayoutConfig) -> Result<(), ServiceError> {
    config.output.validate().map_err(ServiceError::Conflict)?;

    let processing = &config.processing;
    let unsupported = [
        (processing.audio_only, "audio_only"),
        (processing.copy_audio, "copy_audio"),
        (processing.copy_video, "copy_video"),
        (processing.audio_tracks != 1, "audio_tracks != 1"),
        (
            processing.audio_track_index > 0,
            "audio_track_index other than default/first track",
        ),
        (config.text.add_text, "text overlay"),
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
    if config.processing.vtt_enable
        && !Path::new(source).with_extension("vtt").is_file()
        && subtitle_media_path(config, source).is_none()
    {
        warn!(target: Target::file_mail(), channel = config.general.channel_id;
            "WebVTT enabled, but no sidecar or dummy subtitle file found for <span class=\"log-addr\">{source}</span>"
        );
    }

    Ok(())
}

fn subtitle_media_path(config: &PlayoutConfig, source: &str) -> Option<String> {
    if !config.processing.vtt_enable {
        return None;
    }

    let sidecar = Path::new(source).with_extension("vtt");
    if sidecar.is_file() {
        return Some(source.to_string());
    }

    config.processing.vtt_dummy.as_ref().and_then(|dummy| {
        let (dummy_path, _, _) = norm_abs_path(&config.storage.path, dummy).ok()?;
        dummy_path
            .is_file()
            .then(|| dummy_path.to_string_lossy().to_string())
    })
}

fn output_url(config: &PlayoutConfig) -> Result<String, ServiceError> {
    let url = config.output.stream_url.trim();
    if url.is_empty() {
        Err(ServiceError::Conflict(
            "stream output URL must not be empty".to_string(),
        ))
    } else {
        Ok(url.to_string())
    }
}

fn hls_playlist_path(config: &PlayoutConfig) -> Result<PathBuf, ServiceError> {
    let configured_path = config.output.hls_playlist_path.trim();
    if configured_path.is_empty() {
        return Err(ServiceError::Conflict(
            "HLS playlist path must not be empty".to_string(),
        ));
    }

    let (path, _, _) = norm_abs_path(&config.channel.public, configured_path)?;
    let parent = path.parent().ok_or_else(|| {
        ServiceError::Conflict("HLS playlist path must include a parent directory".to_string())
    })?;
    if !parent.is_dir() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(path)
}

/// Resolves the actual playlist file the watchdog should observe. When
/// bitrate variants are configured, ffmpeg renames the base path with a
/// `%v` prefix that it substitutes with the first variant's name (see
/// `ff_engine::resolved_variant_playlist_path`), so `hls_playlist_path`'s
/// literal path is never written to in that case.
fn watchdog_playlist_path(config: &PlayoutConfig) -> Result<PathBuf, ServiceError> {
    let base_path = hls_playlist_path(config)?;
    let variants = config
        .output
        .parsed_hls_variants()
        .map_err(ServiceError::Conflict)?;

    let Some(first_variant) = variants.first() else {
        return Ok(base_path);
    };

    resolved_variant_playlist_path(&base_path.to_string_lossy(), &first_variant.name)
        .map(PathBuf::from)
        .map_err(|e| ServiceError::Conflict(e.to_string()))
}

fn playout_duration(node: &Media) -> Option<f64> {
    (node.out > node.seek).then_some(node.out - node.seek)
}

fn engine_error(error: impl fmt::Display) -> ServiceError {
    ServiceError::Conflict(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::playout_duration;
    use crate::player::utils::Media;

    #[test]
    fn full_placeholder_keeps_explicit_duration() {
        let node = Media {
            out: 12.0,
            duration: 12.0,
            is_placeholder: true,
            ..Media::default()
        };

        assert_eq!(playout_duration(&node), Some(12.0));
    }

    #[test]
    fn shorter_placeholder_slot_is_trimmed() {
        let node = Media {
            out: 5.0,
            duration: 12.0,
            is_placeholder: true,
            ..Media::default()
        };

        assert_eq!(playout_duration(&node), Some(5.0));
    }

    #[test]
    fn regular_clip_keeps_loop_duration() {
        let node = Media {
            out: 20.0,
            duration: 12.0,
            ..Media::default()
        };

        assert_eq!(playout_duration(&node), Some(20.0));
    }
}
