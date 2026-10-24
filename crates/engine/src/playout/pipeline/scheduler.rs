use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::{Rescale, rescale};

use crate::{
    OutputConfig, PlaybackControl,
    input::diagnostics::LiveInputStage,
    output::FrameOutput,
    playout::{
        PlaybackSkipped, Timeline, check_playback_control, synchronize_after_skip,
        synchronize_declared_stream_ends, synchronize_silence_to_video, synchronize_timeline,
    },
    utils::pipeline_queue::{PipelineControl, PipelineQueue},
};

use super::{AudioFrame, Plan, Progress, VideoFrame};

#[allow(clippy::too_many_arguments)]
pub(super) fn coordinate<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    playback_control: &PlaybackControl,
    plan: &Plan,
    video: &PipelineQueue<VideoFrame>,
    audio: &PipelineQueue<AudioFrame>,
    control: &PipelineControl,
    has_embedded_audio: bool,
    progress: &Progress,
) -> Result<()> {
    let mut last_video = None;
    let mut decoded_video = 0;
    let mut decoded_audio = 0;
    let mut video_notified = plan.video.is_none();
    let mut last_progress = Instant::now();
    let mut release_audio = false;
    let mut release_video = false;
    let activity = control.activity();
    playback_control.observe_activity(&activity);
    let result = (|| {
        loop {
            let observed = activity.snapshot();
            check_playback_control(playback_control)?;
            control.check()?;
            // Retain normal A/V interleaving, but do not create a cycle where
            // the demuxer cannot discover EOF because this scheduler refuses
            // to drain the only remaining track. Keep the release until the
            // opposite track supplies another frame.
            let video_front = video.front_timestamp();
            let audio_front = audio.front_timestamp();
            if video_front.is_some() {
                release_audio = false;
            }

            if audio_front.is_some() {
                release_video = false;
            }

            match progress.demux_blocked_track.load(Ordering::Acquire) {
                1 if video_front.is_none() => release_audio = true,
                0 if audio_front.is_none() => release_video = true,
                _ => {}
            }

            let audio_first = match (video_front, audio_front) {
                (Some(video), Some(audio)) => audio < video,
                (None, Some(_)) => true,
                _ => false,
            };
            let mut progressed = false;

            for audio_turn in [audio_first, !audio_first] {
                if audio_turn {
                    let audio_ahead = !timeline.source_timestamp_mode
                        && audio_front
                            .or_else(|| audio.front_timestamp())
                            .is_some_and(|pts| {
                                pts > timeline
                                    .video_pts
                                    .rescale(cfg.video_time_base, rescale::TIME_BASE)
                                    .saturating_add(100_000)
                            });
                    let waiting_first_image =
                        !has_embedded_audio && plan.video.is_some() && decoded_video == 0;

                    if !video.drained() && (waiting_first_image || (audio_ahead && !release_audio))
                    {
                        continue;
                    }

                    if let Some(item) = audio.try_recv()? {
                        output.encode_audio_with_gain_hold(&item.frame, item.hold_gain)?;
                        timeline.audio_pts = timeline
                            .audio_pts
                            .max(item.state.audio_pts + item.frame.samples() as i64);
                        timeline.live_audio_source_us = item.state.live_audio_source_us;
                        decoded_audio += item.frame.samples();
                        progressed = true;
                    }
                } else {
                    let video_ahead = !timeline.source_timestamp_mode
                        && video_front
                            .or_else(|| video.front_timestamp())
                            .is_some_and(|pts| {
                                pts > timeline
                                    .audio_pts
                                    .rescale(cfg.audio_time_base, rescale::TIME_BASE)
                                    .saturating_add(100_000)
                            });

                    if !audio.drained() && video_ahead && !release_video {
                        continue;
                    }

                    if let Some(mut item) = video.try_recv()? {
                        if let Some(logo) = &item.logo {
                            output.apply_shared_logo_overlay(&mut item.frame, logo, item.opacity);
                        } else {
                            output.clear_logo_overlay();
                        }
                        output.encode_video(&item.frame)?;
                        timeline.video_pts = timeline.video_pts.max(item.state.video_pts + 1);
                        timeline.text_pts += 1;
                        timeline.logo_opacity = item.state.logo_opacity;
                        timeline.live_video_source_us = item.state.live_video_source_us;
                        let position_ms = item
                            .frame
                            .pts()
                            .unwrap_or(item.state.video_pts)
                            .saturating_mul(1_000)
                            / i64::from(cfg.fps);
                        output.advance_vtt_subtitles(position_ms)?;
                        last_video = Some(item.frame);
                        decoded_video += 1;

                        if !plan.has_audio && !timeline.source_timestamp_mode {
                            synchronize_silence_to_video(cfg, timeline, output, playback_control)?;
                        }
                        progressed = true;
                    }
                }

                if progressed {
                    break;
                }
            }

            if video.drained() && !video_notified {
                output.video_decoded()?;
                video_notified = true;
            }

            if has_embedded_audio && !timeline.source_timestamp_mode {
                synchronize_declared_stream_ends(
                    cfg,
                    timeline,
                    output,
                    plan.video_padding_end_pts.filter(|_| video.drained()),
                    plan.audio_padding_end_pts.filter(|_| audio.drained()),
                    last_video.as_ref(),
                    playback_control,
                )?;
            } else if plan.video.is_none() && !timeline.source_timestamp_mode {
                synchronize_declared_stream_ends(
                    cfg,
                    timeline,
                    output,
                    Some(timeline.video_pts),
                    None,
                    None,
                    playback_control,
                )?;
            }

            if video.drained() && audio.drained() {
                break;
            }

            if progressed {
                last_progress = Instant::now();
            }

            if !progressed {
                if last_progress.elapsed() > Duration::from_secs(5) {
                    return Err(anyhow!(
                        "input pipeline stalled waiting for selected audio/video tracks"
                    ));
                }

                if let Some(diagnostics) = output.input_diagnostics() {
                    diagnostics.set_stage(LiveInputStage::WaitingForQueue);
                }
                activity.wait(
                    observed,
                    Some(Duration::from_secs(5).saturating_sub(last_progress.elapsed())),
                );
            }
        }

        if decoded_video == 0 && decoded_audio == 0 {
            return Err(anyhow!("input produced no decodable audio or video frames"));
        }

        if !timeline.source_timestamp_mode {
            synchronize_timeline(cfg, timeline, output, last_video.as_ref())?;
        }

        if plan.video.is_some() {
            output.video_finished()?;
        }

        Ok(())
    })();

    if result
        .as_ref()
        .is_err_and(anyhow::Error::is::<PlaybackSkipped>)
    {
        synchronize_after_skip(cfg, timeline, output, last_video.as_ref())?;
    }

    result
}
