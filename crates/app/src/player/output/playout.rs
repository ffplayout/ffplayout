use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{PoisonError, atomic::Ordering},
    time::Duration,
};

use ff_engine::{
    AsyncPlayout, AudioLevelCallback, ClipResult, EncodingConfig, HlsHealth, LiveInputBackend,
    LiveListenerConfig, LogLevel, LogoConfig, LogoFade, OutputConfig, RecordingConfig,
    TextOverlayState,
};
use log::*;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::{
    file::norm_abs_path,
    player::{
        controller::{ChannelManager, loudness_config},
        input::source_generator,
        process_diagnostics::ProcessDiagnostics,
        utils::{Media, sec_to_time},
    },
    utils::{
        config::{ENGINE_AUDIO_SAMPLE_RATE, EncodingSource, OutputMode, PlayoutConfig},
        control::{PlayerCtl, control_state},
        errors::ServiceError,
    },
};

pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    let _running_config = manager.track_running_config(config.clone());
    *manager
        .live_output_config
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    validate_supported_config(&config)?;

    manager
        .audio_effects
        .set_volume(config.audio.volume)
        .map_err(engine_error)?;
    if let Ok(mut audio_level) = manager.audio_level.lock() {
        *audio_level = None;
    }
    let mut output_config = engine_output_config(
        &config,
        manager.audio_effects.clone(),
        manager.live_loudness.clone(),
        manager.loudness_meter.clone(),
        manager.audio_level.clone(),
        manager.text_overlay.clone(),
        desktop_control_callback(manager.clone()),
    )?;
    output_config.output_timing_diagnostics = crate::ARGS.timing_diagnostics;
    let mut moq_attachments = Vec::new();
    manager.output_status.clear();
    output_config.output_status = manager.output_status.clone();
    let parallel = !config.output.discard
        && (config.output.mode != OutputMode::Desktop || !config.output.enabled)
        || !config.outputs.is_empty();
    let moq_demanded = manager.moq_demand.active();
    let playout = if parallel {
        super::instances::open_parallel_playout(
            &config,
            &mut output_config,
            &mut moq_attachments,
            moq_demanded,
            crate::moq::AudioMeterSource::new(
                manager.audio_level.clone(),
                manager.loudness_meter.clone(),
            ),
        )
        .await?
    } else {
        open_playout(&config, output_config.clone()).await?
    };
    manager
        .live_output_config
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(|| config.clone());
    let demand_watcher = manager
        .output_status
        .supports_live_updates()
        .then(|| crate::moq::watch_demand(manager.clone(), moq_demanded));
    *manager.playback_control.lock().await = playout.playback_control();
    if config.output.mode == OutputMode::Desktop {
        #[cfg(feature = "desktop-cpu")]
        info!(channel = config.general.channel_id;
            "Desktop output uses crates/engine winit/softbuffer CPU renderer"
        );
        #[cfg(not(feature = "desktop-cpu"))]
        info!(channel = config.general.channel_id;
            "Desktop output uses crates/engine winit/wgpu renderer"
        );
    }

    let listeners: Vec<_> = config
        .ingest
        .listeners
        .iter()
        .filter(|input| input.enabled && matches!(input.backend.as_str(), "rtmp" | "srt"))
        .map(|input| {
            let backend = if input.backend == "srt" {
                LiveInputBackend::Srt
            } else {
                LiveInputBackend::Rtmp
            };

            Ok(LiveListenerConfig {
                id: input.id,
                priority: input.priority,
                idle_timeout: manager.live_idle_timeout(input.id, input.idle_timeout_ms)?,
                backend,
                url: input.identifier.clone(),
                options: input.options.clone(),
                demuxer_options: input.demuxer_options.clone(),
            })
        })
        .collect::<Result<_, ServiceError>>()?;
    let count = listeners.len();

    if count > 0 {
        playout
            .start_live_listeners(listeners, output_config)
            .await
            .map_err(engine_error)?;
        info!(channel = config.general.channel_id; "Started {count} live listener(s)");
    }

    let result = match (parallel, &config.output.mode) {
        (false, OutputMode::HLS) => play_hls(manager.clone(), &config, &playout).await,
        _ => play_loop(manager.clone(), &config, &playout, None).await,
    };

    drop(demand_watcher);
    let finish_result = playout.finish().await.map_err(engine_error);
    for attachment in moq_attachments {
        if let Err(error) = attachment.finish().await {
            warn!(channel = config.general.channel_id; "MoQ output shutdown: {error}");
        }
    }
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
    let diagnostics = crate::ARGS
        .process_diagnostics
        .then(|| ProcessDiagnostics::new(manager.id, playout.playback_control()));
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

        if let Some(diagnostics) = &diagnostics {
            diagnostics
                .clip_boundary(&config.processing.mode, &node)
                .await;
        }

        // An empty source is intentional when playlist and configured filler
        // are both unavailable: ff-engine then generates black video/silence.
        // Keep that implementation detail out of the player input, but make
        // it explicit in the operator-facing log.
        let display_source = if node.source.is_empty() {
            "dummy-generation"
        } else {
            &node.source
        };

        if node.audio.is_empty() {
            info!(channel = id;
                "Play for <span class=\"log-number\">{}</span>: <span class=\"log-addr\">{}</span>",
                sec_to_time(node.out - node.seek),
                display_source
            );
        } else {
            info!(channel = id;
                "Play for <span class=\"log-number\">{}</span>: <span class=\"log-addr\">{}</span> + audio: <span class=\"log-addr\">{}</span>",
                sec_to_time(node.out - node.seek),
                display_source,
                node.audio
            );
        }

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
        let is_ad = node.ad;
        match playout
            .play_with_timing_logo_fade_rate_and_audio(
                node.source.clone(),
                (node.seek > 0.0).then_some(node.seek),
                duration,
                (!node.audio.is_empty()).then(|| node.audio.clone()),
                subtitle_media_path(config, &node.source),
                LogoFade {
                    fade_in: !is_ad && node.last_ad,
                    fade_out: !is_ad && node.next_ad,
                },
                1.0,
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
                if !node.source.is_empty() {
                    error!(channel = id;
                        "failed while playing {}: {reason}; fallback generated",
                        display_source
                    );
                }
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
        OutputMode::Moq => Err(ServiceError::Conflict(
            "MoQ requires the parallel output path".into(),
        )),
    }
}

#[cfg(any(feature = "desktop", feature = "desktop-cpu"))]
pub(super) async fn open_desktop_playout(
    output_config: OutputConfig,
    fallback_duration: f64,
) -> Result<AsyncPlayout, ServiceError> {
    AsyncPlayout::open_desktop(output_config, fallback_duration)
        .await
        .map_err(engine_error)
}

#[cfg(not(any(feature = "desktop", feature = "desktop-cpu")))]
pub(super) async fn open_desktop_playout(
    _output_config: OutputConfig,
    _fallback_duration: f64,
) -> Result<AsyncPlayout, ServiceError> {
    Err(ServiceError::Conflict(
        "desktop output is not available because ffplayout was built without the desktop feature"
            .to_string(),
    ))
}

pub(super) fn engine_output_config(
    config: &PlayoutConfig,
    audio_effects: ff_engine::AudioEffectsControl,
    live_loudness: ff_engine::LiveLoudnessControl,
    loudness_meter: ff_engine::LoudnessMeterControl,
    audio_level: std::sync::Arc<std::sync::Mutex<Option<ff_engine::AudioLevel>>>,
    text_overlay_state: TextOverlayState,
    desktop_control_callback: ff_engine::DesktopControlCallback,
) -> Result<OutputConfig, ServiceError> {
    let output = config.output.for_playout();
    let width = config
        .program
        .as_ref()
        .map_or(output.width, |program| program.width);
    let height = config
        .program
        .as_ref()
        .map_or(output.height, |program| program.height);
    if width == 0 || height == 0 {
        return Err(ServiceError::Conflict(
            "output size must be greater than zero".to_string(),
        ));
    }
    if !output.fps.is_finite() || output.fps <= 0.0 {
        return Err(ServiceError::Conflict(
            "output fps must be a positive number".to_string(),
        ));
    }
    let fps = config
        .program
        .as_ref()
        .map_or(output.fps, |program| program.fps)
        .round() as u32;

    let logo = config.processing.add_logo.then(|| LogoConfig {
        path: config.processing.logo_path.clone(),
        scale: (!config.processing.logo_scale.trim().is_empty())
            .then(|| config.processing.logo_scale.clone()),
        opacity: config.processing.logo_opacity,
        position: config.processing.logo_position.clone(),
    });
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

    let recording = recording_config(config)?;
    // Source selection is activated at startup, after any pending restart.
    live_loudness.update(
        config.audio.loudness_scope != "off",
        loudness_config(&config.audio),
    );

    Ok(
        OutputConfig::new(width, height, fps, ENGINE_AUDIO_SAMPLE_RATE)
            .with_audio_effects(audio_effects)
            .with_loudness_scope(match config.audio.loudness_scope.as_str() {
                "all" => ff_engine::LoudnessScope::All,
                "live" => ff_engine::LoudnessScope::Live,
                _ => ff_engine::LoudnessScope::Off,
            })
            .with_live_loudness_control(live_loudness)
            .with_loudness_meter_control(loudness_meter)
            .with_audio_level_callback(Some(AudioLevelCallback::new(move |level| {
                if let Ok(mut audio_level) = audio_level.lock() {
                    *audio_level = Some(level);
                }
            })))
            .with_logo(logo)
            .with_text_overlay_state(text_overlay_state)
            .with_desktop_fullscreen(output.desktop_fullscreen)
            .with_desktop_control_callback(desktop_control_callback)
            .with_logging(ffmpeg_log_level, ingest_log_level)
            .with_ffmpeg_ignore_lines(config.logging.ignore_lines.clone())
            .with_channel_id(config.general.channel_id)
            .with_stream_type(output.stream_type.engine_stream_type())
            .with_stream_format(output.stream_format.clone())
            .with_encoding(
                output.video_codec.clone(),
                output.video_options.clone(),
                output.audio_codec.clone(),
                output.audio_options.clone(),
                u64::from(output.audio_bitrate) * 1_000,
            )
            .with_protocol_options(output.protocol_options.clone())
            .with_muxer_options(output.muxer_options.clone())
            .with_metadata_options(output.metadata_options.clone())
            .with_recording(recording),
    )
}

pub(super) fn recording_config(
    config: &PlayoutConfig,
) -> Result<Option<RecordingConfig>, ServiceError> {
    let recording = &config.recording;
    if !recording.enable {
        return Ok(None);
    }

    match recording.source {
        EncodingSource::Stream => {
            if !matches!(config.output.mode, OutputMode::Stream | OutputMode::Moq) {
                return disable_incompatible_recording(
                    config,
                    "source \"stream\" requires stream output",
                );
            }
            if recording.source_output_id != Some(config.output.id) {
                return disable_incompatible_recording(
                    config,
                    "the configured stream source is not the active output",
                );
            }
            Ok(Some(
                RecordingConfig::new(recording.path.clone(), recording.segment_duration)
                    .with_channel_id(Some(config.general.channel_id))
                    .with_retention_days(recording.retention_days)
                    .with_minimum_free_space_gb(recording.minimum_free_space_gb),
            ))
        }
        EncodingSource::HlsVariant => {
            if config.output.mode != OutputMode::HLS {
                return disable_incompatible_recording(
                    config,
                    "source \"hls_variant\" requires HLS output",
                );
            }
            if recording.source_output_id != Some(config.output.id) {
                return disable_incompatible_recording(
                    config,
                    "the configured HLS source is not the active output",
                );
            }
            let stream_index = config
                .output
                .hls_streams()
                .map_err(ServiceError::Conflict)?
                .iter()
                .position(|variant| variant.name == recording.variant)
                .ok_or_else(|| {
                    ServiceError::Conflict("recording HLS variant is unavailable".to_string())
                })?;
            Ok(Some(
                RecordingConfig::new(recording.path.clone(), recording.segment_duration)
                    .with_channel_id(Some(config.general.channel_id))
                    .with_video_stream_index(stream_index)
                    .with_retention_days(recording.retention_days)
                    .with_minimum_free_space_gb(recording.minimum_free_space_gb),
            ))
        }
        EncodingSource::Encode => Ok(Some(
            RecordingConfig::new(recording.path.clone(), recording.segment_duration)
                .with_channel_id(Some(config.general.channel_id))
                .with_retention_days(recording.retention_days)
                .with_minimum_free_space_gb(recording.minimum_free_space_gb)
                .with_encode(EncodingConfig {
                    width: if recording.width == 0 {
                        config
                            .program
                            .as_ref()
                            .map_or(config.output.width, |program| program.width)
                    } else {
                        recording.width
                    },
                    height: if recording.height == 0 {
                        config
                            .program
                            .as_ref()
                            .map_or(config.output.height, |program| program.height)
                    } else {
                        recording.height
                    },
                    video_codec: recording.video_codec.clone(),
                    video_options: recording.video_options.clone(),
                    audio_codec: recording.audio_codec.clone(),
                    audio_options: recording.audio_options.clone(),
                    audio_bitrate: u64::from(recording.audio_bitrate) * 1_000,
                }),
        )),
    }
}

fn disable_incompatible_recording(
    config: &PlayoutConfig,
    reason: &str,
) -> Result<Option<RecordingConfig>, ServiceError> {
    warn!(channel = config.general.channel_id;
        "Recording disabled for this run: {reason}; active output is {}",
        config.output.mode
    );
    Ok(None)
}

pub(super) fn desktop_control_callback(
    manager: ChannelManager,
) -> ff_engine::DesktopControlCallback {
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
    if config.output.discard {
        config
            .output
            .for_playout()
            .validate()
            .map_err(ServiceError::Conflict)?;
    } else {
        config.validate_outputs().map_err(ServiceError::Conflict)?;
    }

    let audio = &config.audio;
    if !audio.volume.is_finite() || !(0.0..=1.5).contains(&audio.volume) {
        return Err(ServiceError::Conflict(
            "audio volume must be between 0.0 and 1.5".to_string(),
        ));
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

pub(super) fn output_url(config: &PlayoutConfig) -> Result<String, ServiceError> {
    let output = config.output.for_playout();
    let url = output.stream_url.trim();
    if url.is_empty() {
        Err(ServiceError::Conflict(
            "stream output URL must not be empty".to_string(),
        ))
    } else {
        Ok(url.to_string())
    }
}

pub(super) fn hls_playlist_path(config: &PlayoutConfig) -> Result<PathBuf, ServiceError> {
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

pub(super) fn engine_error(error: impl fmt::Display) -> ServiceError {
    ServiceError::Conflict(error.to_string())
}

fn request_shutdown(shutdown: &CancellationToken) {
    shutdown.cancel();
}

#[cfg(test)]
mod tests {
    use super::{playout_duration, recording_config, request_shutdown};
    use crate::player::utils::Media;
    use crate::utils::config::{EncodingSource, OutputMode, PlayoutConfig};
    use tokio_util::sync::CancellationToken;

    fn recording_test_config(mode: OutputMode, source: EncodingSource) -> PlayoutConfig {
        let mut config = PlayoutConfig::default();
        config.general.channel_id = 1;
        config.output.id = 3;
        config.output.mode = mode;
        config.output.width = 1280;
        config.output.height = 720;
        config.recording.enable = true;
        config.recording.source = source;
        config.recording.source_output_id = Some(3);
        config.recording.path = "/tmp/recordings".to_string();
        config.recording.segment_duration = 300;
        config.recording.video_codec = "libx264".to_string();
        config.recording.audio_codec = "aac".to_string();
        config.recording.audio_bitrate = 128;
        config
    }

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
    fn desktop_stop_requests_process_shutdown() {
        let shutdown = CancellationToken::new();

        request_shutdown(&shutdown);

        assert!(shutdown.is_cancelled());
    }

    #[test]
    fn desktop_override_disables_stream_copy_recording() {
        let config = recording_test_config(OutputMode::Desktop, EncodingSource::Stream);

        assert!(recording_config(&config).unwrap().is_none());
    }

    #[test]
    fn desktop_override_disables_hls_variant_recording() {
        let config = recording_test_config(OutputMode::Desktop, EncodingSource::HlsVariant);

        assert!(recording_config(&config).unwrap().is_none());
    }

    #[test]
    fn desktop_override_keeps_dedicated_recording_encode() {
        let config = recording_test_config(OutputMode::Desktop, EncodingSource::Encode);

        let recording = recording_config(&config).unwrap().unwrap();
        assert!(recording.encode.is_some());
    }

    #[test]
    fn moq_output_can_copy_its_encoded_packets_to_recording() {
        let config = recording_test_config(OutputMode::Moq, EncodingSource::Stream);
        let recording = recording_config(&config).unwrap().unwrap();
        assert!(recording.encode.is_none());
    }

    #[test]
    fn inactive_copy_source_disables_recording() {
        let mut config = recording_test_config(OutputMode::Stream, EncodingSource::Stream);
        config.recording.source_output_id = Some(2);

        assert!(recording_config(&config).unwrap().is_none());
    }
}
