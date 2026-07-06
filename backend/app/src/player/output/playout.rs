use std::{
    fmt,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, SystemTime},
};

use ff_engine::{
    AsyncPlayout, ClipResult, LogLevel, LogoConfig, LogoFade, OutputConfig, RateControl,
    resolved_variant_playlist_path,
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
        task_runner,
    },
};

pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    validate_supported_config(&config)?;

    manager
        .audio_effects
        .set_volume(config.processing.volume)
        .map_err(engine_error)?;
    let output_config = engine_output_config(&config, manager.audio_effects.clone())?;
    let playout = open_playout(&config, output_config.clone()).await?;
    if config.output.mode == OutputMode::Desktop {
        info!(channel = config.general.channel_id;
            "Desktop output uses backend/engine SDL2 renderer"
        );
    }

    if config.ingest.enable {
        let url = config.ingest.ingest_url.clone();
        playout
            .start_rtmp_live(url.clone(), output_config)
            .await
            .map_err(engine_error)?;
        info!(channel = config.general.channel_id;
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
                error!(channel = channel_id;
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

        info!(channel = id;
            "Play for <span class=\"log-number\">{}</span>: <span class=\"log-addr\">{}</span>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                tokio::spawn(task_runner::run(manager.clone()));
            } else {
                error!(channel = id;
                    "<span class=\"log-cmd\">{:?}</span> executable not exists!",
                    config.task.path
                );
            }
        }

        let duration = playout_duration(&node);
        let is_ad = node.category == "advertisement";
        match playout
            .play_with_timing_and_logo_fade(
                node.source.clone(),
                (node.seek > 0.0).then_some(node.seek),
                duration,
                subtitle_media_path(config, &node.source),
                LogoFade {
                    fade_in: !is_ad && node.last_ad,
                    fade_out: !is_ad && node.next_ad,
                },
            )
            .await
            .map_err(engine_error)?
        {
            ClipResult::LiveEnded => {
                info!(channel = id;
                    "Live input ended; reinitialize playlist at current time"
                );
                manager.list_init.store(true, Ordering::SeqCst);
                node_sources = source_generator(manager.clone()).await;
                continue;
            }
            ClipResult::Played => {}
            ClipResult::Fallback { reason } => {
                error!(channel = id;
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
                info!(channel = id;
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
            let hls_streams = config
                .output
                .hls_streams()
                .map_err(ServiceError::Conflict)?;
            let hls_subtitle = config
                .processing
                .hls_subtitle()
                .map_err(ServiceError::Conflict)?;
            let hls_muxer_streams = if hls_subtitle.is_some() || hls_streams.len() > 1 {
                hls_streams
            } else {
                Vec::new()
            };

            AsyncPlayout::open_hls(
                hls_playlist_path(config)?.to_string_lossy().to_string(),
                output_config,
                fallback_duration,
                hls_muxer_streams,
                hls_subtitle,
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

fn engine_output_config(
    config: &PlayoutConfig,
    audio_effects: ff_engine::AudioEffectsControl,
) -> Result<OutputConfig, ServiceError> {
    let width = config.output.width;
    let height = config.output.height;
    if width == 0 || height == 0 {
        return Err(ServiceError::Conflict(
            "output size must be greater than zero".to_string(),
        ));
    }
    if !config.output.fps.is_finite() || config.output.fps <= 0.0 {
        return Err(ServiceError::Conflict(
            "output fps must be a positive number".to_string(),
        ));
    }
    let fps = config.output.fps.round() as u32;

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
    let ffmpeg_log_level = config
        .logging
        .ffmpeg_level
        .parse::<LogLevel>()
        .map_err(ServiceError::Conflict)?;
    let ingest_log_level = config
        .logging
        .ingest_level
        .parse::<LogLevel>()
        .map_err(ServiceError::Conflict)?;

    Ok(OutputConfig::new(width, height, fps, 48_000)
        .with_audio_effects(audio_effects)
        .with_logo(logo)
        .with_logging(ffmpeg_log_level, ingest_log_level)
        .with_channel_id(config.general.channel_id)
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
    if !processing.volume.is_finite() || !(0.0..=1.0).contains(&processing.volume) {
        return Err(ServiceError::Conflict(
            "processing volume must be between 0.0 and 1.0".to_string(),
        ));
    }
    let unsupported = [(config.text.add_text, "text overlay")];

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
        warn!(channel = config.general.channel_id;
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
    let playlist_name = config.output.hls_playlist_name.trim();
    let relative_path = format!("live/{playlist_name}.m3u8");
    let (path, _, _) = norm_abs_path(&config.channel.public, &relative_path)?;
    let parent = path.parent().ok_or_else(|| {
        ServiceError::Conflict("HLS playlist path must include a parent directory".to_string())
    })?;
    if !parent.is_dir() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(path)
}

/// Resolves the actual playlist file the watchdog should observe. For a
/// standalone HLS output this is the configured playlist path. With a master
/// playlist, the base output is the first rendition and the muxer substitutes
/// its name into the `%v` playlist pattern.
fn watchdog_playlist_path(config: &PlayoutConfig) -> Result<PathBuf, ServiceError> {
    let base_path = hls_playlist_path(config)?;
    let streams = config
        .output
        .hls_streams()
        .map_err(ServiceError::Conflict)?;
    let first_stream = streams
        .first()
        .ok_or_else(|| ServiceError::Conflict("HLS output has no streams".to_string()))?;

    resolved_variant_playlist_path(&base_path.to_string_lossy(), &first_stream.name)
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
