use std::sync::atomic::Ordering;

use ffmpeg_next::{Rescale, rescale};

use crate::{
    PlaybackControl,
    benchmark::{self, Stage},
    clock::SharedPlayoutClock,
    input::diagnostics::LiveInputStage,
    playout::{
        AudioDecoder, ExternalAudioInput, div_ceil, flush_audio_resampler, receive_audio_frames,
        write_silence,
    },
    utils::pipeline_queue::run_worker,
};

use super::{
    FrameSender, NO_TIMESTAMP, WorkerContext, earlier_end, load_timestamp, read_rate::ReadRate,
};

pub(super) fn run(
    context: WorkerContext,
    external_audio: Option<&mut ExternalAudioInput>,
    output_clock: Option<SharedPlayoutClock>,
) {
    let WorkerContext {
        cfg,
        label,
        initial,
        plan,
        control,
        audio_packets,
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

        if let Some(external) = external_audio {
            let video_end = plan
                .video_end_pts
                .map(|pts| pts.rescale(cfg.video_time_base, cfg.audio_time_base));
            let limit = match (plan.audio_limit_pts, video_end) {
                (Some(audio), Some(video)) => Some(audio.min(video)),
                (audio, video) => audio.or(video),
            };
            let mut governor = ReadRate::new(
                output_clock,
                initial
                    .audio_pts
                    .rescale(cfg.audio_time_base, rescale::TIME_BASE),
                initial
                    .audio_pts
                    .rescale(cfg.audio_time_base, rescale::TIME_BASE),
            );

            while limit.is_none_or(|limit| state.audio_pts < limit)
                && (!external.eof || limit.is_some())
            {
                control.check()?;
                let next = state.audio_pts.saturating_add(audio_size.max(1) as i64);
                let next = limit.map_or(next, |limit| next.min(limit));
                external.decode_until(
                    cfg,
                    &mut state,
                    &mut sender,
                    Some(next),
                    &mut decoded,
                    &playback_control,
                )?;
                governor.update(
                    0,
                    Some(
                        state
                            .audio_pts
                            .rescale(cfg.audio_time_base, rescale::TIME_BASE),
                    ),
                    control,
                )?;
            }
        } else if let Some(spec) = &plan.audio {
            let stop = earlier_end(plan.audio_limit_pts, plan.audio_end_pts);
            let mut audio = AudioDecoder::from_spec(
                spec,
                cfg,
                label,
                (plan.seek_us > 0).then_some(plan.source_origin_us.saturating_add(plan.seek_us)),
            )?;

            if !initial.source_timestamp_mode {
                let lead_us = spec
                    .start_us
                    .unwrap_or(plan.source_origin_us)
                    .saturating_sub(plan.source_origin_us)
                    .saturating_sub(plan.seek_us)
                    .max(0);
                let samples =
                    div_ceil(i128::from(lead_us) * i128::from(cfg.sample_rate), 1_000_000) as i64;
                let samples = plan.audio_limit_pts.map_or(samples, |end| {
                    samples.min(end.saturating_sub(state.audio_pts).max(0))
                });
                write_silence(cfg, &mut state, &mut sender, samples)?;
            }

            while let Some(packet) = audio_packets.recv()? {
                state.live_video_source_us = load_timestamp(&progress.video_source_us);
                sender.state = state;

                if let Some(diagnostics) = diagnostics {
                    diagnostics.set_stage(LiveInputStage::ProcessingAudio);
                }
                benchmark::measure(Stage::AudioDecode, || audio.decoder.send_packet(&packet))?;
                receive_audio_frames(
                    &mut audio,
                    &mut state,
                    &mut sender,
                    &mut decoded,
                    stop,
                    plan.media_fade_plan,
                    &playback_control,
                )?;
                progress.audio_source_us.store(
                    state.live_audio_source_us.unwrap_or(NO_TIMESTAMP),
                    Ordering::Relaxed,
                );

                if stop.is_some_and(|limit| state.audio_pts >= limit) {
                    break;
                }
            }

            if stop.is_some_and(|stop| state.audio_pts >= stop) {
                return Ok(());
            }
            audio.decoder.send_eof()?;
            sender.state = state;
            receive_audio_frames(
                &mut audio,
                &mut state,
                &mut sender,
                &mut decoded,
                stop,
                plan.media_fade_plan,
                &playback_control,
            )?;
            flush_audio_resampler(
                &mut audio,
                &mut state,
                &mut sender,
                &mut decoded,
                stop,
                plan.media_fade_plan,
            )?;
        }

        Ok(())
    });
    audio_packets.close();
    audio_frames.close();
}
