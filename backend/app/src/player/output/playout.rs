use std::{
    fmt,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

use ff_engine::{
    AsyncPlayout, AudioLevelCallback, ClipResult, HlsHealth, LogLevel, LogoConfig, LogoFade,
    OutputConfig, TextOverlayState,
};
use log::*;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    file::norm_abs_path,
    player::{
        controller::ChannelManager,
        input::source_generator,
        utils::{Media, get_delta, sec_to_time},
    },
    utils::{
        config::{OutputMode, PlayoutConfig},
        control::{PlayerCtl, control_state},
        errors::ServiceError,
        text::text_config,
    },
};

const HLS_RATE_CORRECTION_DEADBAND_SECONDS: f64 = 0.1;
const HLS_RATE_CORRECTION_MAX_SECONDS: f64 = 2.0;
const HLS_RATE_CORRECTION_MIN_RATE: f64 = 0.98;
const HLS_RATE_CORRECTION_MAX_RATE: f64 = 1.02;
const HLS_RATE_CORRECTION_MAX_DELTA_FACTOR: f64 = 1.0;

pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    validate_supported_config(&config)?;

    manager
        .audio_effects
        .set_volume(config.processing.volume)
        .map_err(engine_error)?;
    if let Ok(mut audio_level) = manager.audio_level.lock() {
        *audio_level = None;
    }
    let output_config = engine_output_config(
        &config,
        manager.audio_effects.clone(),
        manager.audio_level.clone(),
        manager.text_overlay.clone(),
        desktop_control_callback(manager.clone()),
    )?;
    let playout = open_playout(&config, output_config.clone()).await?;
    *manager.playback_control.lock().await = playout.playback_control();
    if config.output.mode == OutputMode::Desktop {
        #[cfg(feature = "desktop-cpu")]
        info!(channel = config.general.channel_id;
            "Desktop output uses backend/engine winit/softbuffer CPU renderer"
        );
        #[cfg(not(feature = "desktop-cpu"))]
        info!(channel = config.general.channel_id;
            "Desktop output uses backend/engine winit/pixels renderer"
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
    let hls_health = playout
        .hls_health()
        .ok_or_else(|| ServiceError::Conflict("HLS output has no health monitor".to_string()))?;
    let watchdog_token = CancellationToken::new();
    let mut watchdog = tokio::spawn(hls_watchdog(
        config.general.channel_id,
        hls_health,
        Duration::from_secs(hls_duration),
        playout.playback_control(),
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
    hls_health: HlsHealth,
    hls_time: Duration,
    playback_control: ff_engine::PlaybackControl,
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

        if cancel_token.is_cancelled() {
            break;
        }

        let age = hls_health.last_muxed_age();
        if age > timeout {
            error!(channel = channel_id;
                "HLS muxer made no progress for <span class=\"log-number\">{:.3}s</span>; restarting playout",
                age.as_secs_f32()
            );
            playback_control.restart_playout();
            return Err(ServiceError::Conflict("HLS muxer stalled".to_string()));
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
                manager.spawn_task_runner().await;
            } else {
                error!(channel = id;
                    "<span class=\"log-cmd\">{:?}</span> executable not exists!",
                    config.task.path
                );
            }
        }

        let duration = playout_duration(&node);
        let playout_rate = if config.output.mode == OutputMode::HLS {
            hls_playout_rate(config, &manager, &node, duration).await
        } else {
            1.0
        };
        let is_ad = node.category == "advertisement";
        match playout
            .play_with_timing_logo_fade_and_rate(
                node.source.clone(),
                (node.seek > 0.0).then_some(node.seek),
                duration,
                subtitle_media_path(config, &node.source),
                LogoFade {
                    fade_in: !is_ad && node.last_ad,
                    fade_out: !is_ad && node.next_ad,
                },
                playout_rate,
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
            ClipResult::Skipped => {
                // Global shutdown also uses `skip_current` to interrupt the
                // decoder. That expected cleanup must not look like a manual
                // skip in the log.
                if manager.is_alive.load(Ordering::SeqCst) {
                    debug!(channel = id; "Skipped current clip by control command");
                }
            }
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
                // channel and request the process-wide shutdown. Returning
                // lets `player()` finish the engine before `main` stops all
                // remaining channels.
                info!(channel = id;
                    "Desktop player window closed; shutting down ffplayout"
                );
                manager.channel.lock().await.active = false;
                request_shutdown(&manager.shutdown);
                return Ok(());
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

async fn hls_playout_rate(
    config: &PlayoutConfig,
    manager: &ChannelManager,
    node: &Media,
    duration: Option<f64>,
) -> f64 {
    if node.seek > 0.0 || manager.list_init.load(Ordering::SeqCst) {
        return 1.0;
    }

    let Some(begin) = node.begin else {
        return 1.0;
    };
    let clip_duration = duration.unwrap_or(node.out - node.seek);
    let (delta, _) = get_delta(config, &begin);
    let time_shift = manager.channel.lock().await.time_shift;
    let shifted_delta = delta - time_shift;
    let rate = hls_rate_correction(shifted_delta, clip_duration);

    if rate != 1.0 {
        debug!(channel = config.general.channel_id;
            "Apply HLS playout rate correction: <span class=\"log-number\">{rate:.5}</span> for delta <span class=\"log-number\">{shifted_delta:.3}</span>s"
        );
    }

    rate
}

fn hls_rate_correction(delta: f64, clip_duration: f64) -> f64 {
    if !delta.is_finite()
        || !clip_duration.is_finite()
        || clip_duration <= 0.0
        || delta.abs() < HLS_RATE_CORRECTION_DEADBAND_SECONDS
        || delta.abs() > clip_duration * HLS_RATE_CORRECTION_MAX_DELTA_FACTOR
    {
        return 1.0;
    }

    let correction = delta.clamp(
        -HLS_RATE_CORRECTION_MAX_SECONDS,
        HLS_RATE_CORRECTION_MAX_SECONDS,
    );
    let corrected_duration = clip_duration + correction;
    if corrected_duration <= 0.0 {
        return 1.0;
    }

    (clip_duration / corrected_duration)
        .clamp(HLS_RATE_CORRECTION_MIN_RATE, HLS_RATE_CORRECTION_MAX_RATE)
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
            AsyncPlayout::open_stream(output_url(config)?, output_config, fallback_duration)
                .await
                .map_err(engine_error)
        }
        OutputMode::Desktop => open_desktop_playout(output_config, fallback_duration).await,
    }
}

#[cfg(any(feature = "desktop", feature = "desktop-cpu"))]
async fn open_desktop_playout(
    output_config: OutputConfig,
    fallback_duration: f64,
) -> Result<AsyncPlayout, ServiceError> {
    AsyncPlayout::open_desktop(output_config, fallback_duration)
        .await
        .map_err(engine_error)
}

#[cfg(not(any(feature = "desktop", feature = "desktop-cpu")))]
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
    audio_level: std::sync::Arc<std::sync::Mutex<Option<ff_engine::AudioLevel>>>,
    text_overlay_state: TextOverlayState,
    desktop_control_callback: ff_engine::DesktopControlCallback,
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
    let text = config
        .text
        .preset
        .as_ref()
        .filter(|preset| preset.use_filename)
        .map(|preset| text_config(preset, None, true));

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
        .with_audio_level_callback(Some(AudioLevelCallback::new(move |level| {
            if let Ok(mut audio_level) = audio_level.lock() {
                *audio_level = Some(level);
            }
        })))
        .with_logo(logo)
        .with_text(text)
        .with_text_overlay_state(text_overlay_state)
        .with_desktop_fullscreen(config.output.desktop_fullscreen)
        .with_desktop_control_callback(desktop_control_callback)
        .with_logging(ffmpeg_log_level, ingest_log_level)
        .with_ffmpeg_ignore_lines(config.logging.ignore_lines.clone())
        .with_channel_id(config.general.channel_id)
        .with_stream_type(config.output.stream_type.engine_stream_type())
        .with_stream_format(config.output.stream_format.clone())
        .with_encoding(
            config.output.video_codec.clone(),
            config.output.video_options.clone(),
            config.output.audio_codec.clone(),
            u64::from(config.output.audio_bitrate) * 1_000,
        ))
}

fn desktop_control_callback(manager: ChannelManager) -> ff_engine::DesktopControlCallback {
    let runtime = tokio::runtime::Handle::current();
    let pool = manager.db_pool.clone();

    ff_engine::DesktopControlCallback::new(move |command| {
        let manager = manager.clone();
        let pool = pool.clone();
        let control = match command {
            ff_engine::DesktopControlCommand::Back => PlayerCtl::Back,
            ff_engine::DesktopControlCommand::Next => PlayerCtl::Next,
            ff_engine::DesktopControlCommand::Reset => PlayerCtl::Reset,
        };

        if manager.is_processing.swap(true, Ordering::SeqCst) {
            return;
        }

        runtime.spawn(async move {
            if let Err(error) = control_state(&pool, &manager, &control).await {
                error!(channel = manager.id; "desktop control failed: {error}");
            }
            manager.is_processing.store(false, Ordering::SeqCst);
        });
    })
}

fn validate_supported_config(config: &PlayoutConfig) -> Result<(), ServiceError> {
    config.output.validate().map_err(ServiceError::Conflict)?;

    let processing = &config.processing;
    if !processing.volume.is_finite() || !(0.0..=1.5).contains(&processing.volume) {
        return Err(ServiceError::Conflict(
            "processing volume must be between 0.0 and 1.5".to_string(),
        ));
    }
    Ok(())
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

fn playout_duration(node: &Media) -> Option<f64> {
    (node.out > node.seek).then_some(node.out - node.seek)
}

fn engine_error(error: impl fmt::Display) -> ServiceError {
    ServiceError::Conflict(error.to_string())
}

fn request_shutdown(shutdown: &CancellationToken) {
    shutdown.cancel();
}

#[cfg(test)]
mod tests {
    use super::{hls_rate_correction, playout_duration, request_shutdown};
    use crate::player::utils::Media;
    use tokio_util::sync::CancellationToken;

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

    #[test]
    fn hls_rate_correction_slows_down_when_playout_is_early() {
        assert!(hls_rate_correction(1.0, 300.0) < 1.0);
    }

    #[test]
    fn hls_rate_correction_speeds_up_when_playout_is_late() {
        assert!(hls_rate_correction(-1.0, 300.0) > 1.0);
    }

    #[test]
    fn hls_rate_correction_ignores_tiny_delta() {
        assert_eq!(hls_rate_correction(0.05, 300.0), 1.0);
    }

    #[test]
    fn hls_rate_correction_is_clamped() {
        assert_eq!(hls_rate_correction(-10.0, 50.0), 1.02);
        assert_eq!(hls_rate_correction(10.0, 50.0), 0.98);
    }

    #[test]
    fn hls_rate_correction_ignores_delta_larger_than_clip() {
        assert_eq!(hls_rate_correction(-645.505, 300.0), 1.0);
        assert_eq!(hls_rate_correction(645.505, 300.0), 1.0);
    }

    #[test]
    fn desktop_stop_requests_process_shutdown() {
        let shutdown = CancellationToken::new();

        request_shutdown(&shutdown);

        assert!(shutdown.is_cancelled());
    }
}
