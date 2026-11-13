use std::{
    collections::{HashSet, VecDeque},
    fmt::Write,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

#[derive(Clone)]
pub(crate) struct Segment {
    pub sequence: u64,
    pub uri: String,
    pub duration: f64,
    pub discontinuity: bool,
    pub date: Option<String>,
}

pub(crate) struct Playlist {
    pub path: PathBuf,
    pub segments: VecDeque<Segment>,
    pub discontinuity_sequence: u64,
    target_duration: u64,
    retired: VecDeque<PathBuf>,
    delete_threshold: usize,
}

impl Playlist {
    pub fn load(path: PathBuf) -> Result<Self> {
        let mut playlist = Self {
            path,
            segments: VecDeque::new(),
            discontinuity_sequence: 0,
            target_duration: 1,
            retired: VecDeque::new(),
            delete_threshold: 1,
        };
        let content = match fs::read_to_string(&playlist.path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(playlist),
            Err(error) => return Err(error).context("reading HLS resume playlist"),
        };

        ensure!(
            content.len() <= 4 * 1024 * 1024,
            "HLS resume playlist is too large"
        );
        ensure!(
            content.starts_with("#EXTM3U"),
            "invalid HLS resume playlist"
        );

        let mut sequence = 0_u64;
        let mut duration = None;
        let mut discontinuity = false;
        let mut date = None;

        for line in content.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                sequence = value.parse()?;
            } else if let Some(value) = line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
                playlist.discontinuity_sequence = value.parse()?;
            } else if let Some(value) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
                playlist.target_duration = value.parse::<u64>()?.max(1);
                ensure!(
                    playlist.target_duration <= 3600,
                    "invalid HLS target duration"
                );
            } else if line == "#EXT-X-DISCONTINUITY" {
                discontinuity = true;
            } else if let Some(value) = line.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:") {
                date = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("#EXTINF:") {
                let value = value.split(',').next().unwrap_or("").parse::<f64>()?;

                ensure!(
                    value.is_finite() && value > 0.0 && value <= 3600.0,
                    "invalid HLS segment duration"
                );
                duration = Some(value);
            } else if !line.is_empty() && !line.starts_with('#') {
                ensure!(
                    safe_name(line),
                    "native HLS resume requires local segment filenames"
                );
                ensure!(
                    playlist.segments.len() < 10_000,
                    "too many HLS resume segments"
                );
                ensure!(
                    playlist.parent().join(line).is_file(),
                    "HLS resume segment is missing: {line}"
                );
                playlist.segments.push_back(Segment {
                    sequence,
                    uri: line.to_owned(),
                    duration: duration.take().context("HLS segment has no duration")?,
                    discontinuity,
                    date: date.take(),
                });
                sequence = sequence.checked_add(1).context("HLS sequence overflow")?;
                discontinuity = false;
            }
        }

        Ok(playlist)
    }

    pub fn parent(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    pub fn configure(
        &mut self,
        segment_seconds: u32,
        delete_threshold: u32,
        family: &str,
        extension: &str,
    ) -> Result<()> {
        let requested_target = u64::from(segment_seconds) + 2;

        if self.segments.is_empty() {
            self.target_duration = requested_target;
        } else {
            ensure!(
                requested_target <= self.target_duration,
                "native HLS segment duration requires a larger TARGETDURATION; start a fresh HLS history before increasing it"
            );
        }
        self.delete_threshold = delete_threshold as usize;
        let mut orphaned = Vec::new();
        let extension = format!(".{extension}");
        let referenced: HashSet<_> = self
            .segments
            .iter()
            .map(|segment| segment.uri.as_str())
            .collect();

        for entry in fs::read_dir(self.parent())? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if let Some(number) = name
                .strip_prefix(family)
                .and_then(|value| value.strip_suffix(&extension))
                .and_then(|value| value.parse::<u64>().ok())
                && !referenced.contains(name.as_ref())
            {
                orphaned.push((number, entry.path()));
            }
        }

        // Preserve the newest unreferenced resources across restarts. Delete
        // excess files only after successfully publishing a fresh playlist.
        orphaned.sort_by_key(|(number, _)| *number);
        self.retired
            .extend(orphaned.into_iter().map(|(_, path)| path));

        Ok(())
    }

    pub fn reset_history(&mut self, discontinuity_sequence: u64) {
        let parent = self.parent().to_owned();

        self.retired.extend(
            self.segments
                .drain(..)
                .map(|segment| parent.join(segment.uri)),
        );
        self.discontinuity_sequence = discontinuity_sequence;
    }

    pub fn validate_duration(&self, duration: f64) -> Result<()> {
        ensure!(
            duration.is_finite()
                && duration > 0.0
                && duration.ceil() as u64 <= self.target_duration,
            "native HLS segment exceeds its fixed target duration"
        );
        Ok(())
    }

    pub fn append(&mut self, segment: Segment, limit: usize) -> Result<()> {
        self.validate_duration(segment.duration)?;
        self.segments.push_back(segment);

        let mut window_duration: f64 = self.segments.iter().map(|segment| segment.duration).sum();
        let minimum_window = self.target_duration as f64 * 3.0;

        // A count limit must not shrink a live window below three target durations.
        // Keep extra entries until removing the oldest still leaves enough media.
        while self.segments.len() > limit {
            let oldest = self
                .segments
                .front()
                .expect("playlist exceeds nonzero limit");

            if window_duration - oldest.duration + 0.000_001 < minimum_window {
                break;
            }

            window_duration -= oldest.duration;

            let old = self
                .segments
                .pop_front()
                .expect("playlist exceeds nonzero limit");

            self.discontinuity_sequence += u64::from(old.discontinuity);
            self.retired.push_back(self.parent().join(old.uri));
        }

        Ok(())
    }

    pub fn publish(&mut self) -> Result<()> {
        let sequence = self.segments.front().map_or(0, |segment| segment.sequence);
        let mut text = String::with_capacity(160 + self.segments.len() * 96);

        write!(
            text,
            "#EXTM3U\n# ffplayout-packager: native\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:{sequence}\n#EXT-X-DISCONTINUITY-SEQUENCE:{}\n",
            self.target_duration, self.discontinuity_sequence
        )?;

        for segment in &self.segments {
            if segment.discontinuity {
                text.push_str("#EXT-X-DISCONTINUITY\n");
            }

            if let Some(date) = &segment.date {
                writeln!(text, "#EXT-X-PROGRAM-DATE-TIME:{date}")?;
            }

            write!(text, "#EXTINF:{:.6},\n{}\n", segment.duration, segment.uri)?;
        }

        atomic_write(&self.path, text.as_bytes())?;

        while self.retired.len() > self.delete_threshold {
            let path = self.retired.pop_front().expect("checked retirement queue");

            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("retiring {}", path.display()));
                }
            }
        }

        Ok(())
    }
}

pub(crate) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .context("HLS resource has no filename")?
        .to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.native.tmp"));

    fs::write(&temp, contents).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("publishing {}", path.display()))?;

    Ok(())
}
