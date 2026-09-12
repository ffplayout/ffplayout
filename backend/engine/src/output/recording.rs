use std::{
    ffi::CString,
    fs,
    path::{Path, PathBuf},
    ptr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use ffmpeg::{codec, format, util::rational::Rational};
use ffmpeg_next as ffmpeg;
use sysinfo::{Disk, Disks};

use crate::{RecordingConfig, utils::ffmpeg::reference_packet};

static RECORDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct RecordingMuxer {
    octx: format::context::Output,
    video_stream_index: usize,
    audio_stream_index: usize,
    monitor: RecordingMonitor,
}

pub(super) struct RecordingMonitor {
    directory: PathBuf,
    channel_id: Option<i32>,
    retention_days: u32,
    minimum_free_space_gb: u32,
    check_interval: Duration,
    next_check: Instant,
}

impl RecordingMuxer {
    pub(super) fn open(
        config: &RecordingConfig,
        video_encoder: &codec::encoder::video::Encoder,
        audio_encoder: &codec::encoder::audio::Encoder,
    ) -> Result<Self> {
        if config.path.trim().is_empty() {
            return Err(anyhow!("recording path must not be empty"));
        }
        if config.segment_duration == 0 {
            return Err(anyhow!(
                "recording segment duration must be greater than zero"
            ));
        }

        let (pattern, monitor) = prepare_recording(config)?;
        let mut octx = segment_output_context(&pattern)?;

        let mut video_stream = octx.add_stream_with(video_encoder)?;
        video_stream.set_time_base(video_encoder.time_base());
        let video_stream_index = video_stream.index();
        let mut audio_stream = octx.add_stream_with(audio_encoder)?;
        audio_stream.set_time_base(audio_encoder.time_base());
        let audio_stream_index = audio_stream.index();

        let mut options = ffmpeg::Dictionary::new();
        options.set("segment_time", &config.segment_duration.to_string());
        options.set("segment_format", "matroska");
        options.set("reset_timestamps", "1");
        options.set("strftime", "1");
        reject_unused_options(octx.write_header_with(options)?)?;

        Ok(Self {
            octx,
            video_stream_index,
            audio_stream_index,
            monitor,
        })
    }

    pub(super) fn write_video(
        &mut self,
        packet: &ffmpeg::Packet,
        time_base: Rational,
    ) -> Result<()> {
        self.write_packet(packet, self.video_stream_index, time_base)
    }

    pub(super) fn write_audio(
        &mut self,
        packet: &ffmpeg::Packet,
        time_base: Rational,
    ) -> Result<()> {
        self.write_packet(packet, self.audio_stream_index, time_base)
    }

    fn write_packet(
        &mut self,
        packet: &ffmpeg::Packet,
        stream_index: usize,
        encoder_time_base: Rational,
    ) -> Result<()> {
        self.monitor.check()?;
        let stream_time_base = self
            .octx
            .stream(stream_index)
            .context("recording output stream is missing")?
            .time_base();
        // Only the packet header is changed below. Keep the encoded payload
        // shared instead of using ffmpeg-next's Clone implementation, which
        // also calls av_packet_make_writable and may copy the payload.
        let mut packet = reference_packet(packet)?;
        packet.set_stream(stream_index);
        packet.rescale_ts(encoder_time_base, stream_time_base);
        packet.write_interleaved(&mut self.octx)?;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.octx.write_trailer()?;
        Ok(())
    }
}

impl RecordingMonitor {
    /// Runs on the same cadence as the free-space check (throttled to once
    /// per `segment_duration`). Retention cleanup failures are logged and
    /// skipped rather than disabling the recording, since a single file
    /// removal error (e.g. a transient permission issue) is not a reason to
    /// stop writing new segments; running low on disk space is, so that
    /// error still propagates and disables the recording as before.
    pub(super) fn check(&mut self) -> Result<()> {
        if (self.retention_days == 0 && self.minimum_free_space_gb == 0)
            || Instant::now() < self.next_check
        {
            return Ok(());
        }
        self.next_check = Instant::now() + self.check_interval;

        if self.retention_days > 0
            && let Err(error) = remove_expired_segments(
                &self.directory,
                self.retention_days,
                self.channel_id.unwrap_or_default(),
            )
        {
            log::error!(channel = self.channel_id.unwrap_or_default(); "Failed to remove expired recording segments in {}: {error}", self.directory.display());
        }

        if self.minimum_free_space_gb == 0 {
            return Ok(());
        }
        ensure_minimum_free_space(&self.directory, self.minimum_free_space_gb)
    }
}

pub(super) fn prepare_recording(config: &RecordingConfig) -> Result<(PathBuf, RecordingMonitor)> {
    let directory = Path::new(&config.path);
    fs::create_dir_all(directory).with_context(|| {
        format!(
            "failed to create recording directory {}",
            directory.display()
        )
    })?;
    remove_expired_segments(
        directory,
        config.retention_days,
        config.channel_id.unwrap_or_default(),
    )?;
    ensure_minimum_free_space(directory, config.minimum_free_space_gb)?;
    let channel_id = config.channel_id.unwrap_or_default();
    let sequence = RECORDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let pattern = directory.join(format!(
        "recording-ch{channel_id}-%Y-%m-%d_%H-%M-%S-{}-{sequence}.mkv",
        std::process::id()
    ));
    let check_interval = Duration::from_secs(u64::from(config.segment_duration));
    Ok((
        pattern,
        RecordingMonitor {
            directory: directory.to_path_buf(),
            channel_id: config.channel_id,
            retention_days: config.retention_days,
            minimum_free_space_gb: config.minimum_free_space_gb,
            check_interval,
            next_check: Instant::now() + check_interval,
        },
    ))
}

/// The segment muxer owns its child files. `ffmpeg::format::output_as` opens
/// the pattern itself as AVIO and would leave an additional literal `%06d`
/// file behind.
pub(super) fn segment_output_context(path: &Path) -> Result<format::context::Output> {
    let path = CString::new(path.to_string_lossy().as_bytes())
        .context("recording output path contains a null byte")?;
    let muxer = CString::new("segment").expect("static segment muxer name is valid");

    unsafe {
        let mut context = ptr::null_mut();
        let result = ffmpeg::ffi::avformat_alloc_output_context2(
            &mut context,
            ptr::null_mut(),
            muxer.as_ptr(),
            path.as_ptr(),
        );
        if result < 0 {
            if !context.is_null() {
                ffmpeg::ffi::avformat_free_context(context);
            }
            return Err(ffmpeg::Error::from(result).into());
        }
        if context.is_null() {
            return Err(ffmpeg::Error::Unknown.into());
        }
        Ok(format::context::Output::wrap(context))
    }
}

fn ensure_minimum_free_space(directory: &Path, minimum_free_space_gb: u32) -> Result<()> {
    if minimum_free_space_gb == 0 {
        return Ok(());
    }

    let directory = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let required = u64::from(minimum_free_space_gb) * 1024 * 1024 * 1024;
    let available = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|disk| directory.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(Disk::available_space)
        .context("could not determine free space for recording path")?;

    if available < required {
        return Err(anyhow!(
            "recording path has {:.2} GB free; at least {} GB is required",
            available as f64 / 1_073_741_824.0,
            minimum_free_space_gb
        ));
    }
    Ok(())
}

fn remove_expired_segments(directory: &Path, retention_days: u32, channel_id: i32) -> Result<()> {
    if retention_days == 0 {
        return Ok(());
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(u64::from(retention_days) * 86_400))
        .unwrap_or(UNIX_EPOCH);
    let channel_prefix = format!("recording-ch{channel_id}-");
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if !is_managed_segment(&path)
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&channel_prefix))
        {
            continue;
        }
        // Prefer the timestamp encoded in the filename over the filesystem
        // mtime: external tools (backups, rsync, archivers, indexers, ...)
        // can rewrite mtime without changing when the segment was actually
        // recorded, which would otherwise make retention unreliable. Legacy
        // segments (no embedded date) still fall back to mtime.
        let recorded_at = match segment_recorded_at(&path) {
            Some(recorded_at) => recorded_at,
            None => entry.metadata()?.modified()?,
        };
        if recorded_at < cutoff {
            fs::remove_file(&path).with_context(|| {
                format!("failed to remove expired recording {}", path.display())
            })?;
        }
    }
    Ok(())
}

/// Extracts the recording start time from a current-format filename
/// (`recording-ch<channel>-%Y-%m-%d_%H-%M-%S-<pid>-<sequence>.mkv`). The
/// timestamp is generated by FFmpeg's `strftime` option using local time, so
/// it is interpreted as local time here too. Returns `None` for legacy
/// filenames or ambiguous/non-existent local times (e.g. a DST transition),
/// in which case the caller falls back to the file's mtime.
fn segment_recorded_at(path: &Path) -> Option<SystemTime> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_prefix("recording-ch")
        .and_then(|name| name.strip_suffix(".mkv"))?;
    let (_channel, started_at, _process_id, _sequence) = split_current_format(stem)?;
    let naive = parse_recording_timestamp(started_at)?;

    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(datetime) => Some(datetime.into()),
        chrono::LocalResult::Ambiguous(..) | chrono::LocalResult::None => None,
    }
}

fn is_managed_segment(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_prefix("recording-ch")
        .and_then(|name| name.strip_suffix(".mkv"))
    else {
        return false;
    };
    is_current_managed_segment(stem) || is_legacy_managed_segment(stem)
}

fn is_current_managed_segment(stem: &str) -> bool {
    let Some((channel, started_at, process_id, sequence)) = split_current_format(stem) else {
        return false;
    };

    channel.parse::<i32>().is_ok()
        && parse_recording_timestamp(started_at).is_some()
        && process_id.chars().all(|ch| ch.is_ascii_digit())
        && !process_id.is_empty()
        && sequence.chars().all(|ch| ch.is_ascii_digit())
        && !sequence.is_empty()
}

/// Splits a current-format segment stem (filename without the
/// `recording-ch` prefix and `.mkv` suffix) into its `(channel, started_at,
/// process_id, sequence)` parts, e.g. `2-2026-08-13_15-42-09-42-0` becomes
/// `("2", "2026-08-13_15-42-09", "42", "0")`.
fn split_current_format(stem: &str) -> Option<(&str, &str, &str, &str)> {
    let mut suffixes = stem.rsplitn(3, '-');
    let sequence = suffixes.next()?;
    let process_id = suffixes.next()?;
    let prefix = suffixes.next()?;
    let (channel, started_at) = prefix.split_once('-')?;
    Some((channel, started_at, process_id, sequence))
}

fn parse_recording_timestamp(value: &str) -> Option<NaiveDateTime> {
    let (date, time) = value.split_once('_')?;
    let date = date
        .split('-')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let time = time
        .split('-')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [year, month, day] = <[u32; 3]>::try_from(date).ok()?;
    let [hour, minute, second] = <[u32; 3]>::try_from(time).ok()?;
    if year < 1970 || hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }

    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)?
        .and_hms_opt(hour, minute, second)
}

fn is_legacy_managed_segment(stem: &str) -> bool {
    let mut parts = stem.split('-');
    let values = [parts.next(), parts.next(), parts.next(), parts.next()];
    parts.next().is_none()
        && values.iter().all(|value| {
            value.is_some_and(|value| {
                !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
            })
        })
        && values[0].is_some_and(|channel| channel.parse::<i32>().is_ok())
        && values[3].is_some_and(|segment| segment.len() == 6)
}

fn reject_unused_options(options: ffmpeg::Dictionary<'_>) -> Result<()> {
    let unused = options
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    if unused.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "unused FFmpeg recording option(s): {}",
            unused.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use crate::RecordingConfig;

    use super::{
        ensure_minimum_free_space, is_managed_segment, prepare_recording, remove_expired_segments,
    };

    #[test]
    fn accepts_disabled_minimum_free_space_limit() {
        assert!(ensure_minimum_free_space(std::path::Path::new("."), 0).is_ok());
    }

    #[test]
    fn rejects_an_unattainable_minimum_free_space_limit() {
        assert!(ensure_minimum_free_space(std::path::Path::new("."), u32::MAX).is_err());
    }

    #[test]
    fn recognizes_only_managed_recording_segments() {
        assert!(is_managed_segment(std::path::Path::new(
            "recording-ch2-2026-08-13_15-42-09-42-0.mkv"
        )));
        assert!(is_managed_segment(std::path::Path::new(
            "recording-ch2-1786617288287263738-42-000001.mkv"
        )));
        assert!(!is_managed_segment(std::path::Path::new("archive.mkv")));
        assert!(!is_managed_segment(std::path::Path::new(
            "recording-ch2-2026-08-13_15-42-09-42-x.mkv"
        )));
    }

    #[test]
    fn retention_does_not_delete_unmanaged_mkv_files() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-retention-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let unmanaged = directory.join("archive.mkv");
        fs::write(&unmanaged, []).unwrap();

        remove_expired_segments(&directory, u32::MAX, 1).unwrap();

        assert!(unmanaged.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retention_uses_the_filename_timestamp_even_if_mtime_is_recent() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-filename-age-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        // The filename says this segment is from the year 2000, but leave
        // mtime at "now" (as if an external tool like a backup or archiver
        // had just touched it). Retention must still honor the filename.
        let expired = directory.join("recording-ch1-2000-01-01_00-00-00-1-0.mkv");
        fs::write(&expired, []).unwrap();

        remove_expired_segments(&directory, 1, 1).unwrap();

        assert!(!expired.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retention_falls_back_to_mtime_for_legacy_segments() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-legacy-age-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        // Legacy filenames (numeric pattern, no embedded date) have no
        // timestamp to parse, so mtime is the only signal available.
        let legacy = directory.join("recording-ch2-1786617288287263738-42-000001.mkv");
        let file = fs::File::create(&legacy).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86_400);
        file.set_modified(old).unwrap();

        remove_expired_segments(&directory, 1, 2).unwrap();

        assert!(!legacy.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn periodic_check_removes_expired_segments_without_disabling_recording() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-periodic-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let expired = directory.join("recording-ch1-2000-01-01_00-00-00-1-0.mkv");
        fs::write(&expired, []).unwrap();

        let mut monitor = super::RecordingMonitor {
            directory: directory.clone(),
            channel_id: Some(1),
            retention_days: 1,
            minimum_free_space_gb: 0,
            check_interval: std::time::Duration::from_secs(1),
            next_check: std::time::Instant::now(),
        };

        assert!(monitor.check().is_ok());
        assert!(!expired.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retention_is_scoped_to_the_channel_at_startup_and_periodically() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-channel-retention-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(2 * 86_400);
        let mut paths = Vec::new();
        for channel in [0, 1, 10, 2] {
            for suffix in ["2000-01-01_00-00-00-1-0", "1786617288287263738-42-000001"] {
                let path = directory.join(format!("recording-ch{channel}-{suffix}.mkv"));
                fs::File::create(&path).unwrap().set_modified(old).unwrap();
                paths.push((channel, path));
            }
        }
        let mut config = RecordingConfig::new(directory.to_string_lossy().into_owned(), 60);
        config.channel_id = Some(1);
        config.retention_days = 1;
        config.minimum_free_space_gb = 0;
        let (_, mut monitor) = prepare_recording(&config).unwrap();
        for (channel, path) in &paths {
            assert_eq!(path.exists(), *channel != 1, "{}", path.display());
            if *channel == 1 {
                fs::File::create(path).unwrap().set_modified(old).unwrap();
            }
        }
        monitor.next_check = std::time::Instant::now();
        monitor.check().unwrap();
        for (channel, path) in &paths {
            assert_eq!(path.exists(), *channel != 1, "{}", path.display());
        }
        // No configured channel uses ch0 in both filenames and retention.
        config.channel_id = None;
        prepare_recording(&config).unwrap();
        for (channel, path) in &paths {
            assert_eq!(
                path.exists(),
                ![0, 1].contains(channel),
                "{}",
                path.display()
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recording_patterns_are_unique_and_include_the_channel() {
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-recording-pattern-{}",
            std::process::id()
        ));
        let config =
            RecordingConfig::new(directory.to_string_lossy(), 300).with_channel_id(Some(7));

        let (first, _) = prepare_recording(&config).unwrap();
        let (second, _) = prepare_recording(&config).unwrap();

        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("recording-ch7-")
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
