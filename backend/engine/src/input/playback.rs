use std::{
    collections::VecDeque,
    mem,
    sync::mpsc::TryRecvError,
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use ffmpeg_next::{
    frame,
    util::{
        channel_layout::ChannelLayout,
        format::sample::{Sample, Type as SampleType},
    },
};
use log::{debug, info, warn};

use crate::{
    PlaybackControl,
    audio_mixer::LiveLoudnessProcessor,
    compositor::logo::LogoOverlay,
    output::FrameOutput,
    playout::check_playback_control,
    utils::ffmpeg::{make_audio_frame_writable, reference_audio_frame, reference_video_frame},
};

use super::live::{
    LIVE_AUDIO_GRACE_SECONDS, LIVE_AUDIO_PTS_JITTER_SECONDS, LIVE_IDLE_TIMEOUT,
    LIVE_SEND_RETRY_INTERVAL, LIVE_STARTUP_TIMEOUT, LiveEnded, LiveEvent, LiveReceiver,
    MAX_LIVE_GAP_SECONDS, MAX_PENDING_AUDIO_FRAMES, trim_audio_start,
};

pub(crate) struct LiveOverrideOutput<'a, O: FrameOutput> {
    pub(super) output: &'a mut O,
    pub(super) live: &'a mut LiveReceiver,
    playback_control: PlaybackControl,
    allow_live_activation: bool,
}

impl<'a, O: FrameOutput> LiveOverrideOutput<'a, O> {
    pub(crate) fn new(
        output: &'a mut O,
        live: &'a mut LiveReceiver,
        playback_control: &PlaybackControl,
    ) -> Self {
        Self {
            output,
            live,
            playback_control: playback_control.clone(),
            allow_live_activation: true,
        }
    }

    pub(super) fn pump_live(&mut self) -> Result<bool> {
        let mut received_event = false;

        loop {
            // Check every event too: a busy live queue may never become empty.
            check_playback_control(&self.playback_control)?;

            match self
                .live
                .pending_event
                .take()
                .map(Ok)
                .unwrap_or_else(|| self.live.rx.try_recv())
            {
                Ok(LiveEvent::Started {
                    session_id,
                    has_audio,
                    listener_id,
                }) => {
                    self.live.session_id = session_id;
                    self.live.listener_id = listener_id;
                    self.live.session_output_start_seconds = None;
                    self.live.session_source_start_seconds = None;
                    self.clear_pending_audio();
                    self.live.last_media_at = Some(Instant::now());
                    self.live.last_audio_at = None;
                    self.live.active = false;
                    self.live.live_session = None;
                    self.live.connecting = true;
                    self.live.source_has_audio = has_audio;
                    self.live.loudness = None;
                    debug!(channel = self.live.channel_id; "live listener #{} connected; waiting for first video frame", self.live.listener_id);
                }
                Ok(LiveEvent::Video(session_id, frame)) => {
                    if session_id == self.live.session_id {
                        if !self.live.active {
                            let Some(session) = self
                                .allow_live_activation
                                .then(|| {
                                    self.playback_control
                                        .try_activate_live_for_listener(self.live.listener_id)
                                })
                                .flatten()
                            else {
                                // Preserve the first frame while a previously accepted
                                // navigation command finishes updating playlist state.
                                self.live.pending_event = Some(LiveEvent::Video(session_id, frame));

                                return Ok(received_event);
                            };
                            self.live.live_session = Some(session);
                            info!(channel = self.live.channel_id; "First video frame from live listener #{} received; switching to live input", self.live.listener_id);
                            self.live.active = true;
                            self.live.connecting = false;
                            self.live.last_audio_at = Some(Instant::now());
                            self.start_live_session(video_seconds(
                                self.live.fps,
                                frame.pts().unwrap_or(0),
                            ));
                        }

                        received_event = true;
                        self.encode_live_video_frame(frame)?;
                        self.flush_pending_audio()?;
                        self.live.last_media_at = Some(Instant::now());
                    }
                }
                Ok(LiveEvent::Audio(session_id, frame)) => {
                    if session_id == self.live.session_id {
                        received_event = true;
                        self.live.last_media_at = Some(Instant::now());

                        if self.live.active {
                            self.encode_live_audio_frame(frame)?;
                        } else if self.live.connecting {
                            let limit = (self.live.sample_rate as usize)
                                .saturating_mul(LIVE_STARTUP_TIMEOUT.as_secs() as usize);
                            let frame = if frame.samples() > limit {
                                trim_audio_start(&frame, frame.samples() - limit)?
                            } else {
                                frame
                            };

                            while !self.live.pending_audio.is_empty()
                                && (self
                                    .live
                                    .pending_audio_samples
                                    .saturating_add(frame.samples())
                                    > limit
                                    || self.live.pending_audio.len() >= MAX_PENDING_AUDIO_FRAMES)
                            {
                                let old = self.live.pending_audio.pop_front().unwrap();
                                self.live.pending_audio_samples -= old.samples();
                            }

                            self.live.pending_audio_samples += frame.samples();
                            self.live.pending_audio.push_back(frame);
                        }
                    }
                }
                Ok(LiveEvent::Ended(session_id)) => {
                    if session_id == self.live.session_id {
                        debug!(channel = self.live.channel_id; "live listener #{} ended; switching back to file playback", self.live.listener_id);

                        if self.live.active {
                            self.fill_live_gap_since_last_media()?;
                            self.align_live_pts_to_common_time();
                            self.prepare_file_resume();
                            self.live.returned_to_file = true;
                        }

                        self.live.active = false;
                        self.live.live_session = None;
                        self.live.connecting = false;
                        self.live.last_audio_at = None;
                        self.clear_pending_audio();
                    }
                }
                Err(TryRecvError::Empty) => return Ok(received_event),
                Err(TryRecvError::Disconnected) => {
                    if self.live.active {
                        self.fill_live_gap_since_last_media()?;
                        self.align_live_pts_to_common_time();
                        self.prepare_file_resume();
                        self.live.returned_to_file = true;
                    }

                    self.live.active = false;
                    self.live.live_session = None;
                    self.live.connecting = false;
                    self.live.last_audio_at = None;
                    self.clear_pending_audio();

                    return Ok(received_event);
                }
            }
        }
    }

    /// Blocks file playback while a live session is active. While the live
    /// input is merely *connecting* (waiting for its first video frame), file
    /// playback keeps running so the output never stalls; the switch happens
    /// as soon as the first live video frame arrives.
    pub(super) fn wait_for_file_playback(&mut self) -> Result<()> {
        self.pump_live()?;

        while self.live.active {
            thread::sleep(Duration::from_millis(10));
            self.pump_live()?;
            self.pad_missing_live_audio()?;
            let idle_for = self
                .live
                .last_media_at
                .map(|last_media_at| last_media_at.elapsed())
                .unwrap_or_default();
            if self.live.active && idle_for >= LIVE_IDLE_TIMEOUT {
                info!(channel = self.live.channel_id; "live listener #{} idle; switching back to file playback", self.live.listener_id);
                self.fill_live_gap(idle_for)?;
                self.align_live_pts_to_common_time();
                self.prepare_file_resume();
                self.live.returned_to_file = true;
                self.live.active = false;
                self.live.live_session = None;
                self.live.connecting = false;
                self.live.last_audio_at = None;
                self.clear_pending_audio();
            }
        }

        if self.live.returned_to_file {
            self.live.returned_to_file = false;

            return Err(LiveEnded.into());
        }

        Ok(())
    }

    fn clear_pending_audio(&mut self) {
        self.live.pending_audio = VecDeque::new();
        self.live.pending_audio_samples = 0;
    }

    /// For streams that announced an audio track, wait for actual audio
    /// frames instead of inserting silence for every video frame. That avoids
    /// repeatedly trimming valid audio merely because the muxer interleaves
    /// video ahead of it. A sustained audio dropout is still padded.
    pub(super) fn pad_missing_live_audio(&mut self) -> Result<()> {
        if !self.live.active || !self.live.source_has_audio {
            return Ok(());
        }

        if self
            .live
            .last_audio_at
            .is_none_or(|since| since.elapsed() < Duration::from_secs_f64(LIVE_AUDIO_GRACE_SECONDS))
        {
            return Ok(());
        }
        self.pad_audio_until(seconds_to_audio_pts(
            self.live.sample_rate,
            video_seconds(self.live.fps, self.live.video_pts),
        ))
    }

    fn fill_live_gap_since_last_media(&mut self) -> Result<()> {
        if let Some(last_media_at) = self.live.last_media_at {
            self.fill_live_gap(last_media_at.elapsed())?;
        }

        Ok(())
    }

    fn fill_live_gap(&mut self, duration: Duration) -> Result<()> {
        // The gap is measured with a wall clock; if the consumer was not
        // pumping for a while (e.g. between clips) it can be arbitrarily
        // large. Cap it so the output never gets stuck writing filler.
        let duration = duration.min(Duration::from_secs_f64(MAX_LIVE_GAP_SECONDS));
        let video_frames = (duration.as_secs_f64() * f64::from(self.live.fps)).ceil() as i64;

        if let Some(last_video_frame) = self.live.last_video_frame.as_ref() {
            let last_video_frame = reference_video_frame(last_video_frame)?;

            for _ in 0..video_frames {
                let mut frame = reference_video_frame(&last_video_frame)?;
                frame.set_pts(Some(self.live.video_pts));
                self.output.encode_video(&frame)?;
                self.remember_video_frame(frame, self.live.video_pts);
                self.live.video_pts += 1;
            }
        } else {
            self.live.video_pts += video_frames;
        }

        let mut remaining_samples =
            (duration.as_secs_f64() * f64::from(self.live.sample_rate)).ceil() as usize;
        let frame_size = self.output.audio_frame_size().max(1);

        while remaining_samples > 0 {
            let samples = remaining_samples.min(frame_size);
            let mut frame = frame::Audio::new(
                Sample::F32(SampleType::Planar),
                samples,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.live.sample_rate);
            frame.set_pts(Some(self.live.audio_pts));

            for channel in 0..2 {
                for sample in frame.plane_mut::<f32>(channel) {
                    *sample = 0.0;
                }
            }
            self.output.encode_audio(&frame)?;
            self.remember_audio_frame_end(self.live.audio_pts + samples as i64);
            remaining_samples -= samples;
        }

        self.live.last_media_at = Some(Instant::now());

        Ok(())
    }

    fn fill_video_until(&mut self, next_pts: i64) -> Result<()> {
        let Some(mut fill_pts) = self.live.last_video_output_pts.map(|pts| pts + 1) else {
            return Ok(());
        };
        let Some(last_video_frame) = self.live.last_video_frame.as_ref() else {
            return Ok(());
        };
        let last_video_frame = reference_video_frame(last_video_frame)?;

        while fill_pts < next_pts {
            let mut frame = reference_video_frame(&last_video_frame)?;
            frame.set_pts(Some(fill_pts));
            self.output.encode_video(&frame)?;
            self.remember_video_frame(frame, fill_pts);
            fill_pts += 1;
        }

        Ok(())
    }

    fn fill_audio_until(&mut self, next_pts: i64) -> Result<()> {
        let mut fill_pts = self
            .live
            .last_audio_output_end_pts
            .unwrap_or(self.live.audio_pts);
        if fill_pts >= next_pts {
            return Ok(());
        }

        let frame_size = self.output.audio_frame_size().max(1);

        while fill_pts < next_pts {
            let samples = (next_pts - fill_pts).min(frame_size as i64) as usize;
            let mut frame = frame::Audio::new(
                Sample::F32(SampleType::Planar),
                samples,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.live.sample_rate);
            frame.set_pts(Some(fill_pts));

            for channel in 0..2 {
                for sample in frame.plane_mut::<f32>(channel) {
                    *sample = 0.0;
                }
            }
            self.output.encode_audio(&frame)?;
            fill_pts += samples as i64;
            self.remember_audio_frame_end(fill_pts);
        }

        Ok(())
    }

    /// Account for an already elapsed audio gap without synchronously pushing
    /// seconds of silence through a realtime desktop queue. Encoded outputs
    /// decline this shortcut and still receive timestamped silence frames.
    fn pad_audio_until(&mut self, next_pts: i64) -> Result<()> {
        let fill_pts = self
            .live
            .last_audio_output_end_pts
            .unwrap_or(self.live.audio_pts);
        let samples = next_pts.saturating_sub(fill_pts);

        if samples > 0 && self.output.pad_audio(samples)? {
            self.remember_audio_frame_end(next_pts);

            return Ok(());
        }
        self.fill_audio_until(next_pts)
    }

    fn remember_video_frame(&mut self, frame: frame::Video, pts: i64) {
        self.live.last_video_frame = Some(frame);
        self.live.last_video_output_pts = Some(pts);
    }

    fn remember_audio_frame_end(&mut self, end_pts: i64) {
        self.live.audio_pts = end_pts;
        self.live.last_audio_output_end_pts = Some(end_pts);
    }

    pub(super) fn start_live_session(&mut self, source_start_seconds: f64) {
        let output_start_seconds = self.common_live_seconds();
        self.live.video_pts = self
            .live
            .video_pts
            .max(seconds_to_video_pts(self.live.fps, output_start_seconds));
        self.live.audio_pts = self.live.audio_pts.max(seconds_to_audio_pts(
            self.live.sample_rate,
            output_start_seconds,
        ));
        self.live.session_output_start_seconds = Some(output_start_seconds);
        self.live.session_source_start_seconds = Some(source_start_seconds);
    }

    fn common_live_seconds(&self) -> f64 {
        let video_seconds = self.live.video_pts as f64 / f64::from(self.live.fps);
        let audio_seconds = self.live.audio_pts as f64 / f64::from(self.live.sample_rate);
        video_seconds.max(audio_seconds)
    }

    fn live_output_seconds(&self, source_seconds: f64) -> f64 {
        let output_start = self
            .live
            .session_output_start_seconds
            .unwrap_or_else(|| self.common_live_seconds());
        let source_start = self
            .live
            .session_source_start_seconds
            .unwrap_or(source_seconds);
        output_start + (source_seconds - source_start)
    }

    pub(super) fn encode_live_video_frame(&mut self, mut frame: frame::Video) -> Result<()> {
        let source_pts = frame.pts().unwrap_or(0);
        let source_seconds = video_seconds(self.live.fps, source_pts);
        let mut pts = seconds_to_video_pts(self.live.fps, self.live_output_seconds(source_seconds));
        // A buggy publisher can jump its PTS forward by minutes or hours
        // mid-stream; bridging that with filler frames would stall the output
        // for the whole gap. Re-anchor the session instead and continue
        // seamlessly. Backward jumps are already handled by the `.max()`
        // floor below and need no filler.
        let max_gap = seconds_to_video_pts(self.live.fps, MAX_LIVE_GAP_SECONDS);

        if pts - self.live.video_pts > max_gap {
            warn!(
                channel = self.live.channel_id;
                "live video pts jumped by {:.3} s; re-anchoring live session",
                video_seconds(self.live.fps, pts - self.live.video_pts)
            );
            self.start_live_session(source_seconds);
            pts = seconds_to_video_pts(self.live.fps, self.live_output_seconds(source_seconds));
        }

        let pts = pts.max(self.live.video_pts);
        self.fill_video_until(pts)?;
        frame.set_pts(Some(pts));

        loop {
            check_playback_control(&self.playback_control)?;
            self.pad_missing_live_audio()?;

            if self.output.try_encode_video(&frame)? {
                break;
            }
            thread::sleep(LIVE_SEND_RETRY_INTERVAL);
        }
        self.remember_video_frame(frame, pts);
        self.live.video_pts = pts + 1;

        if !self.live.source_has_audio {
            self.fill_audio_until(seconds_to_audio_pts(
                self.live.sample_rate,
                video_seconds(self.live.fps, self.live.video_pts),
            ))?;
        }

        Ok(())
    }

    pub(super) fn encode_live_audio_frame(&mut self, mut frame: frame::Audio) -> Result<()> {
        let source_pts = frame.pts().unwrap_or(0);
        let source_seconds = audio_seconds(self.live.sample_rate, source_pts);
        let mut pts = seconds_to_audio_pts(
            self.live.sample_rate,
            self.live_output_seconds(source_seconds),
        );
        let max_gap = seconds_to_audio_pts(self.live.sample_rate, MAX_LIVE_GAP_SECONDS);

        if pts - self.live.audio_pts > max_gap {
            warn!(
                channel = self.live.channel_id;
                "live audio pts jumped by {:.3} s; re-anchoring live session",
                audio_seconds(self.live.sample_rate, pts - self.live.audio_pts)
            );
            self.start_live_session(source_seconds);
            pts = seconds_to_audio_pts(
                self.live.sample_rate,
                self.live_output_seconds(source_seconds),
            );
        }

        let jitter_tolerance =
            seconds_to_audio_pts(self.live.sample_rate, LIVE_AUDIO_PTS_JITTER_SECONDS);
        if pts.abs_diff(self.live.audio_pts) <= jitter_tolerance as u64 {
            pts = self.live.audio_pts;
        }
        // Silence already emitted (or audio preceding the first video) must
        // never shift late samples into the future and introduce A/V drift.
        let overlap = self.live.audio_pts.saturating_sub(pts).max(0) as usize;

        if overlap >= frame.samples() {
            return Ok(());
        }

        if overlap > 0 {
            frame = trim_audio_start(&frame, overlap)?;
            pts += overlap as i64;
        }

        let samples = frame.samples() as i64;
        self.pad_audio_until(pts)?;
        frame.set_pts(Some(pts));
        self.sync_loudness_processor();

        if let Some(loudness) = &mut self.live.loudness {
            make_audio_frame_writable(&mut frame)?;
            loudness.process(&mut frame);
            self.live.loudness_control.set_metrics(loudness.metrics());
        }
        self.output.encode_audio(&frame)?;
        self.remember_audio_frame_end(pts + samples);
        self.live.last_audio_at = Some(Instant::now());

        Ok(())
    }

    fn flush_pending_audio(&mut self) -> Result<()> {
        let pending = mem::take(&mut self.live.pending_audio);
        self.live.pending_audio_samples = 0;

        for frame in pending {
            self.encode_live_audio_frame(frame)?;
        }

        Ok(())
    }

    fn sync_loudness_processor(&mut self) {
        let settings = self.live.loudness_control.settings();

        if !settings.enabled {
            self.live.loudness = None;

            return;
        }

        if let Some(processor) = &mut self.live.loudness {
            if processor.config() != settings.config {
                processor.update_config(settings.config);
            }
        } else {
            let channel_id = self.live.channel_id;
            self.live.loudness = LiveLoudnessProcessor::new(self.live.sample_rate, settings.config)
                .map_err(|error| {
                    warn!(channel = channel_id; "live loudness normalization disabled: {error}");
                    error
                })
                .ok();
        }
    }

    fn align_live_pts_to_common_time(&mut self) {
        let common_seconds = self.common_live_seconds();
        self.live.video_pts = self
            .live
            .video_pts
            .max(seconds_to_video_pts(self.live.fps, common_seconds));
        self.live.audio_pts = self
            .live
            .audio_pts
            .max(seconds_to_audio_pts(self.live.sample_rate, common_seconds));
    }

    fn prepare_file_resume(&mut self) {
        let video_seconds = self.live.video_pts as f64 / f64::from(self.live.fps);
        let audio_seconds = self.live.audio_pts as f64 / f64::from(self.live.sample_rate);
        self.live.file_resume_at_seconds = Some(video_seconds.max(audio_seconds));
        self.live.file_resume_shift_seconds = None;
    }

    fn file_video_pts(&mut self, source_pts: i64) -> i64 {
        resume_pts(
            self.live.fps,
            self.live.file_resume_at_seconds,
            &mut self.live.file_resume_shift_seconds,
            source_pts,
            self.live.video_pts,
        )
    }

    fn file_audio_pts(&mut self, source_pts: i64) -> i64 {
        resume_pts(
            self.live.sample_rate,
            self.live.file_resume_at_seconds,
            &mut self.live.file_resume_shift_seconds,
            source_pts,
            self.live.audio_pts,
        )
    }
}

/// Shared resume-pts computation for both video and audio.
///
/// `resume_shift_seconds` is intentionally a single value shared between the
/// video and audio streams: whichever stream resumes first fixes the shift,
/// and the other stream reuses it so both tracks stay aligned to the same
/// point in the file.
pub(super) fn resume_pts(
    rate: u32,
    resume_at_seconds: Option<f64>,
    resume_shift_seconds: &mut Option<f64>,
    source_pts: i64,
    floor_pts: i64,
) -> i64 {
    if let Some(resume_seconds) = resume_at_seconds {
        let source_seconds = source_pts as f64 / f64::from(rate);
        let shift_seconds = *resume_shift_seconds.get_or_insert(resume_seconds - source_seconds);
        ((source_seconds + shift_seconds) * f64::from(rate)).round() as i64
    } else {
        source_pts.max(floor_pts)
    }
    .max(floor_pts)
}

fn video_seconds(fps: u32, pts: i64) -> f64 {
    pts as f64 / f64::from(fps)
}

fn audio_seconds(sample_rate: u32, pts: i64) -> f64 {
    pts as f64 / f64::from(sample_rate)
}

fn seconds_to_video_pts(fps: u32, seconds: f64) -> i64 {
    seconds_to_pts(fps, seconds)
}

fn seconds_to_audio_pts(sample_rate: u32, seconds: f64) -> i64 {
    seconds_to_pts(sample_rate, seconds)
}

pub(super) fn seconds_to_pts(rate: u32, seconds: f64) -> i64 {
    let ticks = seconds * f64::from(rate);
    let nearest = ticks.round();
    // Preserve exact ticks after a floating-point seconds round trip. Plain
    // ceil can turn e.g. frame 7 into frame 8 and permanently advance video.
    if (ticks - nearest).abs() <= f64::EPSILON * ticks.abs().max(1.0) * 4.0 {
        nearest as i64
    } else {
        ticks.ceil() as i64
    }
}

impl<O: FrameOutput> FrameOutput for LiveOverrideOutput<'_, O> {
    fn audio_frame_size(&self) -> usize {
        self.output.audio_frame_size()
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        if !self.live.active
            && let Some(pts) = frame.pts()
        {
            self.live.video_pts = self.live.video_pts.max(pts);
        }
        self.wait_for_file_playback()?;

        let mut frame = reference_video_frame(frame)?;
        let pts = self.file_video_pts(frame.pts().unwrap_or(self.live.video_pts));
        self.fill_video_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_video(&frame)?;
        self.remember_video_frame(frame, pts);
        self.live.video_pts = pts + 1;

        Ok(())
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if !self.live.active
            && let Some(pts) = frame.pts()
        {
            self.live.audio_pts = self.live.audio_pts.max(pts);
        }
        self.wait_for_file_playback()?;

        let mut frame = reference_audio_frame(frame)?;
        let samples = frame.samples() as i64;
        let pts = self.file_audio_pts(frame.pts().unwrap_or(self.live.audio_pts));
        self.fill_audio_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_audio(&frame)?;
        self.remember_audio_frame_end(pts + samples);

        Ok(())
    }

    fn reset_after_skip(&mut self, _video_pts: i64, _audio_pts: i64) -> Result<bool> {
        // Finish skip cleanup before allowing a new live takeover. Otherwise
        // cleanup padding itself could enter the live wait loop.
        self.allow_live_activation = false;
        // The suspended playlist timeline is not being sent to the output.
        // Padding it would enter wait_for_file_playback again after the skip
        // flag was consumed, preventing shutdown until the live source ends.
        if self.live.active {
            return Ok(true);
        }

        let common_seconds = self.common_live_seconds();
        let video_pts = seconds_to_video_pts(self.live.fps, common_seconds);
        let audio_pts = seconds_to_audio_pts(self.live.sample_rate, common_seconds);

        if !self.output.reset_after_skip(video_pts, audio_pts)? {
            return Ok(false);
        }

        self.live.video_pts = video_pts;
        self.live.audio_pts = audio_pts;
        self.live.last_video_frame = None;
        self.live.last_video_output_pts = None;
        self.live.last_audio_output_end_pts = Some(audio_pts);

        Ok(true)
    }

    fn apply_logo_overlay(
        &mut self,
        frame: &mut frame::Video,
        logo: &LogoOverlay,
        opacity_factor: f64,
    ) {
        self.output.apply_logo_overlay(frame, logo, opacity_factor);
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        self.output.benchmarks_logo_overlay()
    }

    fn set_video_end(&mut self, video_end_pts: Option<i64>) -> Result<()> {
        self.output.set_video_end(video_end_pts)
    }

    fn video_finished(&mut self) -> Result<()> {
        self.output.video_finished()
    }

    fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        self.output
            .write_vtt_subtitles(media_path, output_start_ms, source_start_ms)
    }

    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        self.output.clear_vtt_subtitles()
    }

    fn advance_vtt_subtitles(&mut self, output_position_ms: i64) -> Result<()> {
        self.output.advance_vtt_subtitles(output_position_ms)
    }
}
