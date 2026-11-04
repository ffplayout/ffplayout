use std::sync::atomic::Ordering;
#[cfg(test)]
use std::thread;

use crate::{
    PlaybackControl,
    benchmark::{self, Stage},
    input::diagnostics::LiveInputStage,
    playout::{
        VideoDecoder, black_video_frame_for_config, div_ceil, encode_composited_frame,
        finish_video, receive_video_frames, repeat_single_video_frame_to_limit,
    },
    utils::pipeline_queue::run_worker,
};

use super::{FrameSender, NO_TIMESTAMP, StreamSpec, WorkerContext, earlier_end, load_timestamp};

pub(super) fn run(context: WorkerContext, spec: &StreamSpec) {
    let WorkerContext {
        cfg,
        label,
        initial,
        plan,
        control,
        video_packets,
        video_frames,
        audio_frames,
        progress,
        benchmark,
        diagnostics,
        audio_size,
        ..
    } = context;

    if let Some(benchmark) = benchmark {
        benchmark::activate(benchmark.clone());
    }
    run_worker(control, || {
        let mut video = VideoDecoder::from_spec(
            spec,
            cfg,
            label,
            (plan.seek_us > 0).then_some(plan.source_origin_us.saturating_add(plan.seek_us)),
            initial.video_pts,
            initial.text_pts,
            plan.video_limit_pts.or(plan.video_end_pts),
        )?;
        #[cfg(test)]
        thread::sleep(std::time::Duration::from_millis(spec.decode_delay_ms));
        let stop = earlier_end(plan.video_limit_pts, plan.video_end_pts);
        let mut state = initial;
        let mut decoded = 0;
        let playback_control = PlaybackControl::default();
        let mut sender = FrameSender {
            texts: Default::default(),
            video: video_frames,
            audio: audio_frames,
            state,
            audio_size,
            cfg,
            diagnostics,
            opacity: 1.0,
        };

        if !initial.source_timestamp_mode {
            let lead_us = spec
                .start_us
                .unwrap_or(plan.source_origin_us)
                .saturating_sub(plan.source_origin_us)
                .saturating_sub(plan.seek_us)
                .max(0);
            let lead_frames = div_ceil(i128::from(lead_us) * i128::from(cfg.fps), 1_000_000) as i64;
            let end = plan
                .video_limit_pts
                .unwrap_or(i64::MAX)
                .min(initial.video_pts.saturating_add(lead_frames));

            while state.video_pts < end {
                encode_composited_frame(
                    black_video_frame_for_config(cfg),
                    &mut video,
                    &mut state,
                    plan.media_fade_plan,
                    plan.logo_fade_plan,
                    &mut sender,
                    &mut decoded,
                    None,
                )?;
            }
        }

        while let Some(packet) = video_packets.recv()? {
            state.live_audio_source_us = load_timestamp(&progress.audio_source_us);
            sender.state = state;

            if let Some(diagnostics) = diagnostics {
                diagnostics.set_stage(LiveInputStage::ProcessingVideo);
            }
            benchmark::measure(Stage::VideoDecode, || video.decoder.send_packet(&packet))?;
            receive_video_frames(
                cfg,
                None,
                &mut video,
                &mut state,
                &mut sender,
                &mut decoded,
                stop,
                plan.logo_fade_plan,
                plan.media_fade_plan,
                &playback_control,
            )?;
            progress.video_source_us.store(
                state.live_video_source_us.unwrap_or(NO_TIMESTAMP),
                Ordering::Relaxed,
            );

            if stop.is_some_and(|limit| state.video_pts >= limit) {
                break;
            }
        }
        sender.state = state;

        if stop.is_some_and(|stop| state.video_pts >= stop) {
            repeat_single_video_frame_to_limit(
                cfg,
                None,
                &mut Some(video),
                &mut state,
                &mut sender,
                &mut decoded,
                &mut 0,
                plan.video_limit_pts,
                plan.logo_fade_plan,
                plan.media_fade_plan,
                &playback_control,
                false,
                None,
            )?;
        } else {
            finish_video(
                cfg,
                None,
                &mut Some(video),
                &mut state,
                &mut sender,
                &mut decoded,
                &mut 0,
                &mut false,
                plan.video_limit_pts,
                plan.logo_fade_plan,
                plan.media_fade_plan,
                &playback_control,
                false,
                None,
            )?;
        }

        Ok(())
    });
    video_packets.close();
    video_frames.close();
}
