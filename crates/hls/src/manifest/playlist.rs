use std::{
    collections::{HashSet, VecDeque},
    fmt::Write,
    fs,
    io::{ErrorKind, Write as _},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, ensure};
use jiff::{SignedDuration, Timestamp};

#[derive(Clone)]
pub(crate) struct Segment {
    pub sequence: u64,
    pub uri: String,
    pub initialization: Option<String>,
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
    pub missing_resources: bool,
    longest_window: f64,
    evicted: Vec<Segment>,
    cleanup_failed: bool,
    initialization_files: Vec<(u64, PathBuf)>,
    replaced_resources: Option<(Duration, HashSet<PathBuf>)>,
}

impl Playlist {
    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            segments: VecDeque::new(),
            discontinuity_sequence: 0,
            target_duration: 1,
            retired: VecDeque::new(),
            delete_threshold: 1,
            missing_resources: false,
            longest_window: 0.0,
            evicted: Vec::new(),
            cleanup_failed: false,
            initialization_files: Vec::new(),
            replaced_resources: None,
        }
    }

    pub fn load(path: PathBuf) -> Result<Self> {
        let mut playlist = Self::empty(path);
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
        let mut initialization = None;

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
            } else if let Some(value) = line.strip_prefix("#EXT-X-MAP:") {
                let uri = value
                    .strip_prefix("URI=\"")
                    .and_then(|value| value.strip_suffix('"'))
                    .context("invalid native HLS initialization map")?;
                ensure!(safe_name(uri), "invalid HLS initialization resource: {uri}");
                playlist.check_resource(uri)?;
                initialization = Some(uri.to_owned());
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
                playlist.check_resource(line)?;
                playlist.segments.push_back(Segment {
                    sequence,
                    uri: line.to_owned(),
                    initialization: initialization.clone(),
                    duration: duration.take().context("HLS segment has no duration")?,
                    discontinuity,
                    date: date.take(),
                });
                sequence = sequence.checked_add(1).context("HLS sequence overflow")?;
                discontinuity = false;
            }
        }

        playlist.longest_window = playlist
            .segments
            .iter()
            .map(|segment| segment.duration)
            .sum();

        Ok(playlist)
    }

    fn check_resource(&mut self, uri: &str) -> Result<()> {
        let path = self.parent().join(uri);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Err(error) if error.kind() == ErrorKind::NotFound => self.missing_resources = true,
            Ok(_) => anyhow::bail!("invalid HLS resume resource: {}", path.display()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        }

        Ok(())
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
        replacing: bool,
    ) -> Result<()> {
        let requested_target = u64::from(segment_seconds);
        ensure!(
            self.segments.iter().all(|segment| {
                segment
                    .uri
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| ext == extension)
                    && segment.initialization.is_some() == (extension == "m4s")
            }),
            "native HLS segment format changed; start a fresh HLS history"
        );
        if extension == "m4s" {
            ensure!(
                self.segments.iter().all(|segment| {
                    segment
                        .initialization
                        .as_deref()
                        .and_then(|uri| resource_number(uri, family, ".init.mp4"))
                        .zip(resource_number(&segment.uri, family, ".m4s"))
                        .is_some_and(|(init, fragment)| init <= fragment)
                }),
                "native HLS fragment or initialization filename belongs to another rendition"
            );
        }

        if self.segments.is_empty() {
            self.target_duration = requested_target;
        } else {
            ensure!(
                requested_target <= self.target_duration,
                "native HLS segment duration requires a larger TARGETDURATION; start a fresh HLS history before increasing it"
            );
        }
        self.delete_threshold = delete_threshold as usize;
        if replacing {
            return Ok(());
        }
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

            if extension == ".m4s"
                && let Some(number) = resource_number(&name, family, ".init.mp4")
            {
                self.initialization_files.push((number, entry.path()));
            }

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

    pub fn last_discontinuity_sequence(&self) -> Result<u64> {
        let count = self.segments.iter().filter(|s| s.discontinuity).count() as u64;

        self.discontinuity_sequence
            .checked_add(count)
            .context("HLS discontinuity sequence overflow")
    }

    pub fn reset_history(&mut self, discontinuity_sequence: u64) {
        if !self.segments.is_empty() {
            let grace = crate::retention::grace(self.segments.iter().map(|s| s.duration));
            let resources = self
                .segments
                .iter()
                .flat_map(|segment| {
                    std::iter::once(&segment.uri).chain(segment.initialization.iter())
                })
                .map(|uri| self.parent().join(uri))
                .collect();
            self.replaced_resources = Some((grace, resources));
        }
        let parent = self.parent().to_owned();

        self.retired.extend(
            self.segments
                .drain(..)
                .map(|segment| parent.join(segment.uri)),
        );
        self.discontinuity_sequence = discontinuity_sequence;
    }

    pub fn overlaps_program_date_time(&self, next: Timestamp) -> Result<bool> {
        let mut start = None;

        for segment in &self.segments {
            if let Some(date) = &segment.date {
                start = Some(date.parse::<Timestamp>()?);
            }
            if let Some(date) = start {
                let duration =
                    SignedDuration::try_from(Duration::try_from_secs_f64(segment.duration)?)?;
                let end = date.checked_add(duration)?;

                if end > next {
                    return Ok(true);
                }

                // Undated segments after an anchor inherit its time progression.
                start = Some(end);
            }
        }

        Ok(false)
    }

    pub fn validate_duration(&self, duration: f64) -> Result<()> {
        ensure!(
            duration.is_finite()
                && duration > 0.0
                && duration.round() as u64 <= self.target_duration,
            "native HLS segment exceeds its fixed target duration"
        );
        Ok(())
    }

    pub fn append(&mut self, segment: Segment) -> Result<()> {
        self.validate_duration(segment.duration)?;
        if let Some(uri) = &segment.initialization
            && self
                .segments
                .back()
                .and_then(|last| last.initialization.as_ref())
                != Some(uri)
            && let Some(number) = filename_number(uri)
        {
            let path = self.parent().join(uri);
            if !self
                .initialization_files
                .iter()
                .any(|(_, entry)| *entry == path)
            {
                self.initialization_files.push((number, path));
            }
        }
        self.segments.push_back(segment);

        Ok(())
    }

    pub fn trim_start(&self, limit: usize) -> Option<u64> {
        let mut window_duration: f64 = self.segments.iter().map(|segment| segment.duration).sum();
        let minimum_window = self.target_duration as f64 * 3.0;
        let mut remaining = self.segments.iter();

        // A count limit must not shrink a live window below three target durations.
        while let Some(oldest) = remaining.next() {
            if remaining.len() < limit {
                return Some(oldest.sequence);
            }
            if window_duration - oldest.duration + 0.000_001 < minimum_window {
                return Some(oldest.sequence);
            }
            window_duration -= oldest.duration;
        }

        None
    }

    pub fn trim_before(&mut self, first: u64) {
        while let Some(old) = self
            .segments
            .pop_front_if(|segment| segment.sequence < first)
        {
            self.discontinuity_sequence += u64::from(old.discontinuity);
            self.retired.push_back(self.parent().join(&old.uri));
            self.evicted.push(old);
        }
    }

    pub fn stage(&mut self, shared_audio: bool) -> Result<StagedFile> {
        let sequence = self.segments.front().map_or(0, |segment| segment.sequence);
        let version = if self
            .segments
            .iter()
            .any(|segment| segment.initialization.is_some())
        {
            7
        } else {
            3
        };
        let mut text = String::with_capacity(160 + self.segments.len() * 96);

        write!(
            text,
            "#EXTM3U\n# ffplayout-packager: native\n#EXT-X-VERSION:{version}\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:{sequence}\n#EXT-X-DISCONTINUITY-SEQUENCE:{}\n",
            self.target_duration, self.discontinuity_sequence
        )?;

        if shared_audio {
            writeln!(text, "{}", crate::SHARED_AUDIO_MARKER)?;
        }

        let mut initialization = None;
        for segment in &self.segments {
            if segment.discontinuity {
                text.push_str("#EXT-X-DISCONTINUITY\n");
            }

            if let Some(date) = &segment.date {
                writeln!(text, "#EXT-X-PROGRAM-DATE-TIME:{date}")?;
            }
            if let Some(uri) = &segment.initialization
                && initialization != Some(uri)
            {
                writeln!(text, "#EXT-X-MAP:URI=\"{uri}\"")?;
                initialization = Some(uri);
            }

            write!(text, "#EXTINF:{:.6},\n{}\n", segment.duration, segment.uri)?;
        }

        if let Some((grace, resources)) = &self.replaced_resources {
            let until = SystemTime::now() + *grace;
            for resource in resources {
                crate::retention::protect(resource, until, &self.path)?;
            }
        }
        let now = SystemTime::now();

        for segment in &self.evicted {
            let grace = Duration::from_secs_f64(self.longest_window + segment.duration);
            let until = now
                .checked_add(grace)
                .context("invalid HLS retention deadline")?;

            crate::retention::protect(&self.parent().join(&segment.uri), until, &self.path)?;

            if let Some(initialization) = &segment.initialization {
                crate::retention::protect(&self.parent().join(initialization), until, &self.path)?;
            }
        }

        StagedFile::new(&self.path, text.as_bytes())
    }

    pub fn committed(&mut self) {
        let duration = self
            .segments
            .iter()
            .map(|segment| segment.duration)
            .sum::<f64>();
        self.longest_window = self.longest_window.max(duration);
        self.replaced_resources = None;
        self.evicted.clear();
    }

    pub fn retire(&mut self) {
        let result = self.retire_resources();

        if let Err(error) = &result
            && !self.cleanup_failed
        {
            log::warn!(
                "Could not retire HLS resources for {}: {error:#}",
                self.path.display()
            );
        }

        self.cleanup_failed = result.is_err();
    }

    fn retire_resources(&mut self) -> Result<()> {
        let mut index = 0;
        let mut first_error = None;
        let now = SystemTime::now();

        while self.retired.len() > index + self.delete_threshold {
            let path = &self.retired[index];

            match crate::retention::remove(path, now) {
                Ok(true) => {
                    self.retired.remove(index);
                }
                Ok(false) => break,
                Err(error) => {
                    first_error.get_or_insert(error);
                    index += 1;
                }
            }
        }

        // An init file belongs to every numbered fragment until the next init.
        // Keep it as long as any of those fragments can still be fetched.
        if !self.initialization_files.is_empty()
            && let Some(oldest) = self
                .segments
                .front()
                .and_then(|segment| filename_number(&segment.uri))
            && let oldest = self
                .retired
                .iter()
                .filter_map(|path| filename_number(path.file_name()?.to_str()?))
                .fold(oldest, u64::min)
            && let Some(keep_from) = self
                .initialization_files
                .iter()
                .map(|(number, _)| *number)
                .filter(|number| *number <= oldest)
                .max()
        {
            let mut index = 0;
            while let Some((number, path)) = self.initialization_files.get(index) {
                let still_referenced = self.segments.iter().any(|segment| {
                    segment.initialization.as_deref()
                        == path.file_name().and_then(|name| name.to_str())
                });

                if *number < keep_from
                    && !still_referenced
                    && crate::retention::remove(path, SystemTime::now())?
                {
                    self.initialization_files.swap_remove(index);
                } else {
                    index += 1;
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error.into());
        }

        Ok(())
    }
}

fn resource_number(name: &str, family: &str, extension: &str) -> Option<u64> {
    name.strip_prefix(family)?
        .strip_suffix(extension)?
        .parse()
        .ok()
}

fn filename_number(name: &str) -> Option<u64> {
    let stem = name
        .strip_suffix(".init.mp4")
        .or_else(|| name.strip_suffix(".m4s"))?;
    stem.rsplit_once('_')?.1.parse().ok()
}

pub(crate) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

pub(crate) struct StagedFile {
    target: PathBuf,
    temporary: tempfile::NamedTempFile,
}

impl StagedFile {
    pub fn new(path: &Path, contents: &[u8]) -> Result<Self> {
        let parent = path.parent().context("HLS resource has no directory")?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(".hls-");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            builder.permissions(fs::Permissions::from_mode(0o666));
        }

        let mut temporary = builder
            .tempfile_in(parent)
            .with_context(|| format!("staging {}", path.display()))?;

        temporary.write_all(contents)?;
        temporary.as_file().sync_all()?;

        Ok(Self {
            target: path.to_owned(),
            temporary,
        })
    }

    pub fn publish(self, replace: bool) -> Result<()> {
        let result = if replace {
            self.temporary.persist(&self.target)
        } else {
            self.temporary.persist_noclobber(&self.target)
        };

        result.with_context(|| format!("publishing {}", self.target.display()))?;

        let directory = self
            .target
            .parent()
            .context("HLS resource has no directory")?;

        crate::sync_directory(directory)
            .with_context(|| format!("synchronizing HLS directory {}", directory.display()))?;

        Ok(())
    }
}
