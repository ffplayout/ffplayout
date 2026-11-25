use std::{
    collections::HashSet,
    ffi::CString,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, ensure};
use ffmpeg_next as ffmpeg;

use crate::utils::{
    config::{HlsSubtitle, HlsVariant},
    ffmpeg_capabilities::ffmpeg_capabilities,
};

const HLS_RESUME_MAX_AGE: Duration = Duration::from_secs(60);

pub(super) fn playlist_path(path: &str, variants: &[HlsVariant]) -> Result<String> {
    if variants.is_empty() {
        return Ok(path.to_string());
    }

    let path = Path::new(path);
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .context("HLS playlist path must include a file name")?;
    Ok(path
        .with_file_name("%v.m3u8")
        .to_string_lossy()
        .into_owned())
}

/// Resolves the concrete, on-disk playlist path ffmpeg writes for a single
/// variant once it substitutes `%v` (produced by [`playlist_path`]) with the
/// variant's `name` from `var_stream_map`. Callers (e.g. a watchdog that
/// checks the playlist's mtime) need this to know which file to observe when
/// real bitrate variants are configured.
pub fn resolved_variant_playlist_path(path: &str, variant_name: &str) -> Result<String> {
    let path = Path::new(path);
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .context("HLS playlist path must include a file name")?;
    Ok(path
        .with_file_name(format!("{variant_name}.m3u8"))
        .to_string_lossy()
        .into_owned())
}

pub(super) fn validate_variants(variants: &[HlsVariant]) -> Result<()> {
    let mut names = HashSet::new();

    for variant in variants {
        if variant.name == "master" {
            return Err(anyhow!("HLS variant name \"master\" is reserved"));
        }

        if !names.insert(variant.name.as_str()) {
            return Err(anyhow!("duplicate HLS variant name {}", variant.name));
        }
    }

    Ok(())
}

pub(super) fn output_context(path: &str) -> Result<ffmpeg::format::context::Output> {
    let path = CString::new(path).context("HLS output path contains a null byte")?;
    let format = CString::new("hls").expect("static HLS format name is valid");

    unsafe {
        let mut context = ptr::null_mut();
        let result = ffmpeg::ffi::avformat_alloc_output_context2(
            &mut context,
            ptr::null_mut(),
            format.as_ptr(),
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

        Ok(ffmpeg::format::context::Output::wrap(context))
    }
}

/// Files stay present so every process locks the same inode, including after a crash.
/// Closing these handles releases ownership; lock files must never be unlinked.
pub(super) struct HlsSession {
    _locks: Vec<File>,
    media_playlists: Vec<String>,
    owned_playlists: Vec<String>,
    master_playlist: Option<String>,
    pending_media: Vec<PathBuf>,
    next_retirement: Instant,
}

impl HlsSession {
    pub(super) fn acquire(
        media_playlists: &[String],
        master_playlist: Option<&str>,
        subtitles: bool,
    ) -> Result<Self> {
        let mut paths = cleanup_playlist_paths(media_playlists, master_playlist)?;
        if master_playlist.is_none()
            && let Some(first) = media_playlists.first()
        {
            let master = master_playlist_path(first);
            let master = master.to_string_lossy();
            let children = child_playlist_paths(&master)?;
            if !children.is_empty()
                && (media_playlists.iter().any(|p| p == master.as_ref())
                    || children.iter().any(|path| media_playlists.contains(path)))
            {
                paths.extend(children);
                paths.push(master.into_owned());
            }
        }

        // FFmpeg attaches the single subtitle track to the first rendition only.
        // Reserve its path even before the first master has been written.
        if let Some(playlist) = media_playlists.first() {
            let path = Path::new(playlist);
            let stem = path.file_stem().context("HLS playlist has no file stem")?;
            let subtitle_path = path.with_file_name(format!("{}_vtt.m3u8", stem.to_string_lossy()));

            if subtitles || subtitle_path.exists() {
                paths.push(subtitle_path.to_string_lossy().into_owned());
            }
        }

        paths.sort();
        paths.dedup();
        let owned_playlists = paths.clone();

        let mut lock_paths = paths
            .into_iter()
            .map(|path| {
                let path = Path::new(&path);
                let name = path.file_name().context("HLS playlist has no file name")?;
                let parent = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let parent = fs::canonicalize(parent).with_context(|| {
                    format!("failed to resolve HLS directory {}", parent.display())
                })?;

                Ok(parent.join(format!(".{}.lock", name.to_string_lossy())))
            })
            .collect::<Result<Vec<_>>>()?;

        lock_paths.sort();
        lock_paths.dedup();
        let mut locks = Vec::new();

        for lock_path in lock_paths {
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .with_context(|| format!("failed to open HLS lock {}", lock_path.display()))?;

            lock.try_lock().with_context(|| {
                format!(
                    "HLS playlist is already in use or cannot be locked: {}",
                    lock_path.display()
                )
            })?;
            locks.push(lock);
        }

        let referenced = referenced_segments(&owned_playlists)?;
        let owners: HashSet<_> = owned_playlists
            .iter()
            .filter_map(|p| Path::new(p).file_name())
            .collect();
        let mut pending_media = Vec::new();
        if let Some(parent) = common_playlist_parent(&owned_playlists) {
            for entry in fs::read_dir(parent)? {
                let marker = entry?.path();
                if let Some(path) = ff_hls::retention::marked_resource(&marker)
                    && let Some(name) = path.file_name().and_then(|name| name.to_str())
                    && !referenced.contains(name)
                    && segment_family(name).is_some()
                {
                    let owner = match fs::read_to_string(&marker) {
                        Ok(owner) => owner,
                        Err(error) if error.kind() == ErrorKind::NotFound => continue,
                        Err(error) => return Err(error.into()),
                    };
                    if owners.contains(std::ffi::OsStr::new(&owner)) {
                        pending_media.push(path);
                    }
                }
            }
        }
        Ok(Self {
            pending_media,
            next_retirement: Instant::now(),
            _locks: locks,
            media_playlists: media_playlists.to_vec(),
            owned_playlists,
            master_playlist: master_playlist.map(str::to_owned),
        })
    }

    pub fn retire_pending_media(&mut self) -> Result<()> {
        if self.pending_media.is_empty() {
            return Ok(());
        }
        let now = Instant::now();
        if now >= self.next_retirement {
            self.next_retirement = now + Duration::from_secs(1);
            retire_media(&mut self.pending_media, SystemTime::now())?;
        }
        Ok(())
    }

    pub(super) fn requires_replacement(&self, native: Option<bool>) -> Result<bool> {
        if self.master_playlist.is_none()
            && self.owned_playlists.iter().any(|path| {
                !self.media_playlists.contains(path)
                    && Path::new(path)
                        .file_name()
                        .is_some_and(|name| name == "master.m3u8")
            })
        {
            return Ok(true);
        }
        let mut latest = None;

        for path in &self.owned_playlists {
            let content = match fs::read_to_string(path) {
                Ok(content) => content,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("reading HLS playlist {path}"));
                }
            };
            if self.master_playlist.is_none()
                && self.media_playlists.contains(path)
                && content
                    .lines()
                    .any(|line| line.starts_with("#EXT-X-STREAM-INF:"))
            {
                return Ok(true);
            }
            latest = latest.max(fresh_playlist_modified_time(path)?);
            if !content.lines().any(|line| line.starts_with("#EXTINF:")) {
                continue;
            }
            let was_native = content
                .lines()
                .any(|line| line == "# ffplayout-packager: native");
            if was_native != native.is_some() {
                return Ok(true);
            }
            if let Some(fragmented) = native
                && content.lines().filter_map(playlist_uri).any(|uri| {
                    if fragmented {
                        uri.ends_with(".ts")
                    } else {
                        uri.ends_with(".m4s")
                    }
                })
            {
                return Ok(true);
            }
        }

        Ok(latest.is_some_and(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default()
                > HLS_RESUME_MAX_AGE
        }))
    }

    pub(super) fn resume_start_number(&self) -> Result<Option<u64>> {
        prune_unreferenced_segments(&self.owned_playlists)?;
        media_sequence_start_number(&self.media_playlists)
    }

    pub fn staged_resume(&self, subtitles: bool, start: u64) -> Result<Option<StagedHls>> {
        for path in &self.media_playlists {
            match fs::read_to_string(path) {
                Ok(text)
                    if text
                        .lines()
                        .any(|line| line.starts_with("#EXT-X-DISCONTINUITY-SEQUENCE:")) =>
                {
                    return StagedHls::resume(self, subtitles, start).map(Some);
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    pub(super) fn replacement(&self, subtitles: bool) -> Result<HlsReplacement> {
        let mut retained = self
            .media_playlists
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if subtitles && let Some(playlist) = retained.first() {
            let stem = playlist
                .file_stem()
                .context("HLS playlist has no file stem")?;
            retained.push(playlist.with_file_name(format!("{}_vtt.m3u8", stem.to_string_lossy())));
        }
        if let Some(master) = &self.master_playlist {
            retained.push(PathBuf::from(master));
        }
        let referenced = referenced_segments(&self.owned_playlists)?;
        let mut published_media = referenced_segment_paths(&self.owned_playlists)?;
        published_media.sort();
        published_media.dedup();
        let mut media = published_media.clone();
        let families = segment_families(&self.owned_playlists, &referenced)?;
        if let Some(parent) = common_playlist_parent(&self.owned_playlists) {
            for entry in fs::read_dir(parent)? {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_str()
                    .and_then(segment_family)
                    .is_some_and(|family| families.contains(family))
                    && is_hls_segment(&entry.path())
                {
                    media.push(entry.path());
                }
            }
        }
        let obsolete_playlists = self
            .owned_playlists
            .iter()
            .map(PathBuf::from)
            .filter(|path| !retained.contains(path))
            .collect();

        let mut next_sequence = 0;
        let mut grace = Duration::ZERO;
        let mut discontinuity_sequence = 0;
        for path in &self.owned_playlists {
            let content = match fs::read_to_string(path) {
                Ok(content) => content,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let segments = content
                .lines()
                .filter(|line| line.starts_with("#EXTINF:"))
                .count() as u64;

            if segments > 0 {
                let after_last = media_sequence(&content)
                    .unwrap_or_default()
                    .checked_add(segments)
                    .context("HLS media sequence overflow")?;
                next_sequence = next_sequence.max(after_last);
            }
            let durations = content.lines().filter_map(|line| {
                line.strip_prefix("#EXTINF:")?
                    .split(',')
                    .next()?
                    .parse()
                    .ok()
            });
            grace = grace.max(ff_hls::retention::grace(durations));
            let base = content
                .lines()
                .find_map(|line| line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:"))
                .map(str::parse::<u64>)
                .transpose()?
                .unwrap_or(0);
            let count = content
                .lines()
                .filter(|line| *line == "#EXT-X-DISCONTINUITY")
                .count() as u64;
            discontinuity_sequence = discontinuity_sequence.max(
                base.checked_add(count)
                    .context("HLS discontinuity sequence overflow")?,
            );
        }
        media.sort();
        media.dedup();
        Ok(HlsReplacement {
            discontinuity_sequence,
            owner: PathBuf::from(
                self.master_playlist
                    .as_ref()
                    .unwrap_or(&self.media_playlists[0]),
            ),
            published_media,
            grace,
            next_check: None,
            media,
            obsolete_playlists,
            next_sequence,
        })
    }
}

pub(in crate::output) struct HlsReplacement {
    media: Vec<PathBuf>,
    obsolete_playlists: Vec<PathBuf>,
    pub next_sequence: u64,
    pub discontinuity_sequence: u64,
    grace: Duration,
    owner: PathBuf,
    published_media: Vec<PathBuf>,
    next_check: Option<SystemTime>,
}

impl HlsReplacement {
    pub fn protect(&self, now: SystemTime) -> Result<()> {
        let until = now
            .checked_add(self.grace)
            .context("invalid HLS retention duration")?;
        for path in &self.published_media {
            ff_hls::retention::protect(path, until, &self.owner)?;
        }
        Ok(())
    }

    pub fn retire(&mut self, now: SystemTime) -> Result<bool> {
        let first = self.next_check.is_none();
        if first {
            for path in &self.obsolete_playlists {
                remove_playlist(path)?;
            }
        } else if self.next_check.is_some_and(|deadline| now < deadline) {
            return Ok(false);
        }
        retire_media(&mut self.media, now)?;
        let delay = if first {
            self.grace
        } else {
            Duration::from_secs(1)
        };
        self.next_check = now.checked_add(delay);
        Ok(self.media.is_empty())
    }
}

fn retire_media(paths: &mut Vec<PathBuf>, now: SystemTime) -> Result<()> {
    let mut i = 0;
    while i < paths.len() {
        if ff_hls::retention::remove(&paths[i], now)? {
            paths.swap_remove(i);
        } else {
            i += 1;
        }
    }
    Ok(())
}

// FFmpeg updates individual manifests as it closes renditions. On replacement
// run those writes against private names, keeping segments at their public
// paths. Mirror the new manifests once every rendition has media.
pub(super) struct StagedHls {
    prefix: String,
    playlists: Vec<StagedPlaylist>,
    replacement: Option<HlsReplacement>,
    next_check: Instant,
    start_number: u64,
    discontinuity_base: u64,
}

struct StagedPlaylist {
    staged: PathBuf,
    public: PathBuf,
    modified: Option<SystemTime>,
    history: Option<SegmentHistory>,
}

struct SegmentHistory {
    first: u64,
    base: u64,
    segments: Vec<(String, u64)>,
}

impl SegmentHistory {
    fn read(text: &str) -> Result<Self> {
        let first = media_sequence(text).unwrap_or_default();
        let base = text
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:"))
            .map(str::parse::<u64>)
            .transpose()?
            .unwrap_or_default();
        let mut generation = base;
        let mut segments = Vec::new();
        for line in text.lines() {
            if line == "#EXT-X-DISCONTINUITY" {
                generation = generation
                    .checked_add(1)
                    .context("HLS discontinuity sequence overflow")?;
            } else if !line.is_empty() && !line.starts_with('#') {
                segments.push((line.to_owned(), generation));
            }
        }
        Ok(Self {
            first,
            base,
            segments,
        })
    }

    // append_list reconstructs retained entries but discards their discontinuity
    // base and can regenerate subtitle URIs. Restore both by media sequence.
    fn restore(&self, text: &str, new_generation: u64) -> Result<String> {
        let seq = media_sequence(text).context("missing HLS media sequence")?;
        let offset = usize::try_from(
            seq.checked_sub(self.first)
                .context("HLS media sequence moved backwards")?,
        )?;
        let mut previous = if offset == 0 && !self.segments.is_empty() {
            self.base
        } else {
            offset
                .checked_sub(1)
                .and_then(|n| self.segments.get(n))
                .map_or(new_generation, |(_, generation)| *generation)
        };
        let mut history = self.segments.iter().skip(offset).peekable();
        let mut result = String::with_capacity(text.len() + 64);

        for line in text.lines() {
            if line == "#EXT-X-DISCONTINUITY" || line.starts_with("#EXT-X-DISCONTINUITY-SEQUENCE:")
            {
                continue;
            }

            if line == "#EXTM3U" {
                writeln!(result, "{line}\n#EXT-X-DISCONTINUITY-SEQUENCE:{previous}")?;
            } else if line.starts_with("#EXTINF:") {
                let generation = history
                    .peek()
                    .map_or(new_generation, |(_, generation)| *generation);
                if generation != previous {
                    ensure!(
                        previous.checked_add(1) == Some(generation),
                        "inconsistent HLS discontinuity history"
                    );
                    result.push_str("#EXT-X-DISCONTINUITY\n");
                    previous = generation;
                }
                writeln!(result, "{line}")?;
            } else if !line.is_empty() && !line.starts_with('#') {
                writeln!(
                    result,
                    "{}",
                    history.next().map_or(line, |(uri, _)| uri.as_str())
                )?;
            } else {
                writeln!(result, "{line}")?;
            }
        }

        Ok(result)
    }
}

impl StagedHls {
    pub fn new(session: &HlsSession, subtitles: bool, replacement: HlsReplacement) -> Result<Self> {
        let mut staged = Self::open(session, subtitles, replacement.next_sequence)?;
        staged.discontinuity_base = replacement.discontinuity_sequence;
        staged.replacement = Some(replacement);
        Ok(staged)
    }

    fn resume(session: &HlsSession, subtitles: bool, start: u64) -> Result<Self> {
        let mut staged = Self::open(session, subtitles, start)?;
        let mut last_segment = None;
        for playlist in &mut staged.playlists {
            if session.master_playlist.as_deref().map(Path::new) == Some(playlist.public.as_path())
            {
                continue;
            }
            let text = match fs::read_to_string(&playlist.public) {
                Ok(text) => text,
                Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
                Err(error) => return Err(error.into()),
            };
            let history = SegmentHistory::read(&text)?;
            let Some((_, generation)) = history.segments.last() else {
                return Self::new(session, subtitles, session.replacement(subtitles)?);
            };
            let end = (history.segments.len(), *generation);
            if history.first != start
                || last_segment
                    .replace(end)
                    .is_some_and(|previous| previous != end)
            {
                // append_list starts all renditions at the same counter.
                return Self::new(session, subtitles, session.replacement(subtitles)?);
            }

            staged.discontinuity_base = *generation;
            fs::write(&playlist.staged, text)?;
            // A seed is input for append_list, never a newly published window.
            playlist.modified = Some(fs::metadata(&playlist.staged)?.modified()?);
            playlist.history = Some(history);
        }
        Ok(staged)
    }

    fn open(session: &HlsSession, subtitles: bool, start_number: u64) -> Result<Self> {
        let prefix = format!(".hls-{}-", jiff::Timestamp::now().as_microsecond());
        let mut public = session
            .media_playlists
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if subtitles && let Some(first) = public.first() {
            let stem = first
                .file_stem()
                .context("HLS playlist has no stem")?
                .to_string_lossy();
            public.push(first.with_file_name(format!("{stem}_vtt.m3u8")));
        }
        if let Some(master) = &session.master_playlist {
            public.push(PathBuf::from(master));
        }
        let playlists = public
            .into_iter()
            .map(|path| {
                let staged = staged_path(&path, &prefix);
                StagedPlaylist {
                    staged,
                    public: path,
                    modified: None,
                    history: None,
                }
            })
            .collect();

        Ok(Self {
            discontinuity_base: 0,
            prefix,
            playlists,
            replacement: None,
            next_check: Instant::now(),
            start_number,
        })
    }

    pub fn path(&self, path: &str) -> String {
        staged_path(Path::new(path), &self.prefix)
            .to_string_lossy()
            .into_owned()
    }

    pub fn master_name(&self) -> String {
        format!("{}master.m3u8", self.prefix)
    }

    pub fn is_replacement(&self) -> bool {
        self.replacement.is_some()
    }

    pub fn start_number(&self) -> u64 {
        self.start_number
    }

    pub fn publish(&mut self, force: bool) -> Result<()> {
        let now = Instant::now();
        if !force && now < self.next_check {
            return Ok(());
        }
        self.next_check = now + Duration::from_millis(250);
        let mut updates = Vec::new();

        for (index, playlist) in self.playlists.iter().enumerate() {
            let StagedPlaylist {
                staged,
                public,
                modified: previous,
                history,
            } = playlist;
            let Some(modified) = fresh_playlist_modified_time(&staged.to_string_lossy())? else {
                continue;
            };
            if Some(modified) == *previous {
                continue;
            }
            let mut content = fs::read_to_string(staged)?;
            let master = content
                .lines()
                .any(|line| line.starts_with("#EXT-X-STREAM-INF:"));
            if master {
                content = content.replace(&self.prefix, "");
            } else if !content.lines().any(|line| line.starts_with("#EXTINF:")) {
                continue;
            } else if let Some(history) = history {
                content = history.restore(
                    &content,
                    self.discontinuity_base
                        .checked_add(1)
                        .context("HLS discontinuity sequence overflow")?,
                )?;
            } else {
                let sequence =
                    media_sequence(&content).context("missing staged HLS media sequence")?;
                let base = if sequence == self.start_number {
                    self.discontinuity_base
                } else {
                    self.discontinuity_base
                        .checked_add(1)
                        .context("HLS discontinuity sequence overflow")?
                };
                // FFmpeg's subtitle playlist can omit discont_start. All
                // tracks enter the same generation, even after it slides out.
                let mut header = format!("#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:{base}\n");
                if sequence == self.start_number
                    && !content.lines().any(|line| line == "#EXT-X-DISCONTINUITY")
                {
                    header.push_str("#EXT-X-DISCONTINUITY\n");
                }
                content = content.replacen("#EXTM3U\n", &header, 1);
            }
            updates.push((index, public.clone(), modified, content));
        }

        if let Some(previous) = &self.replacement
            && previous.next_check.is_none()
        {
            if updates.len() != self.playlists.len() {
                return Ok(());
            }
            previous.protect(SystemTime::now())?;
        }
        for (_, public, _, content) in &updates {
            let temp = staged_path(public, ".hls-publish-");
            fs::write(&temp, content)?;
            fs::rename(&temp, public)?;
        }
        for (index, _, modified, _) in updates {
            self.playlists[index].modified = Some(modified);
        }
        if let Some(replacement) = &mut self.replacement
            && replacement.retire(SystemTime::now())?
        {
            self.replacement = None;
        }

        Ok(())
    }
}

impl Drop for StagedHls {
    fn drop(&mut self) {
        for playlist in &self.playlists {
            let _ = remove_playlist(&playlist.staged);
        }
    }
}

fn staged_path(path: &Path, prefix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{prefix}{name}"))
}

fn cleanup_playlist_paths(
    media_playlists: &[String],
    master_playlist: Option<&str>,
) -> Result<Vec<String>> {
    let mut paths = media_playlists.iter().cloned().collect::<HashSet<_>>();

    if let Some(master_playlist) = master_playlist {
        paths.insert(master_playlist.to_string());

        for playlist in child_playlist_paths(master_playlist)? {
            paths.insert(playlist);
        }
    }

    Ok(paths.into_iter().collect())
}

fn prune_unreferenced_segments(media_playlists: &[String]) -> Result<()> {
    let referenced_segments = referenced_segments(media_playlists)?;
    let segment_families = segment_families(media_playlists, &referenced_segments)?;
    let Some(parent) = common_playlist_parent(media_playlists) else {
        return Ok(());
    };

    for entry in fs::read_dir(&parent)
        .with_context(|| format!("failed to read HLS directory {}", parent.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", parent.display()))?;
        let path = entry.path();

        if !is_hls_segment(&path) {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if segment_family(file_name).is_some_and(|family| segment_families.contains(family))
            && !referenced_segments.contains(file_name)
        {
            ff_hls::retention::remove(&path, SystemTime::now())?;
        }
    }

    Ok(())
}

fn fresh_playlist_modified_time(path: &str) -> Result<Option<SystemTime>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to stat HLS playlist {path}"));
        }
    };

    metadata
        .modified()
        .map(Some)
        .with_context(|| format!("failed to read mtime for HLS playlist {path}"))
}

fn referenced_segment_paths(media_playlists: &[String]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for playlist in media_playlists {
        let Some(parent) = Path::new(playlist).parent() else {
            continue;
        };

        paths.extend(
            playlist_segment_entries(playlist)?
                .into_iter()
                .filter(|uri| !uri.contains(['/', '\\']))
                .map(|uri| parent.join(uri)),
        );
    }

    Ok(paths)
}

fn referenced_segments(media_playlists: &[String]) -> Result<HashSet<String>> {
    let mut segments = HashSet::new();

    for playlist in media_playlists {
        for segment in playlist_segment_entries(playlist)? {
            if let Some(file_name) = Path::new(&segment)
                .file_name()
                .and_then(|name| name.to_str())
            {
                segments.insert(file_name.to_string());
            }
        }
    }

    Ok(segments)
}

fn playlist_segment_entries(path: &str) -> Result<Vec<String>> {
    let playlist = match fs::read_to_string(path) {
        Ok(playlist) => playlist,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read HLS playlist {path}"));
        }
    };

    Ok(playlist
        .lines()
        .filter_map(|line| {
            playlist_uri(line)
                .filter(|uri| {
                    Path::new(uri)
                        .extension()
                        .is_some_and(|ext| ext == "ts" || ext == "vtt" || ext == "m4s")
                })
                .or_else(|| initialization_uri(line))
        })
        .map(ToOwned::to_owned)
        .collect())
}

fn initialization_uri(line: &str) -> Option<&str> {
    line.strip_prefix("#EXT-X-MAP:")?;
    playlist_attribute_uri(line, "URI")
        .filter(|uri| !uri.contains(['/', '\\']) && segment_family(uri).is_some())
}

fn child_playlist_paths(master_playlist: &str) -> Result<Vec<String>> {
    let playlist = match fs::read_to_string(master_playlist) {
        Ok(playlist) => playlist,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read HLS master {master_playlist}"));
        }
    };
    let parent = Path::new(master_playlist).parent().unwrap_or(Path::new(""));

    Ok(playlist
        .lines()
        .filter_map(|line| playlist_uri(line).or_else(|| playlist_attribute_uri(line, "URI")))
        .filter(|entry| {
            !entry.contains(['/', '\\'])
                && Path::new(entry)
                    .extension()
                    .is_some_and(|ext| ext == "m3u8")
        })
        .map(|entry| parent.join(entry).to_string_lossy().into_owned())
        .collect())
}

fn playlist_attribute_uri<'a>(line: &'a str, attribute: &str) -> Option<&'a str> {
    let value = line
        .strip_prefix('#')?
        .split_once(':')?
        .1
        .split(',')
        .find_map(|entry| entry.trim().strip_prefix(&format!("{attribute}=\"")))?;
    value.strip_suffix('"')
}

fn segment_families(
    media_playlists: &[String],
    referenced_segments: &HashSet<String>,
) -> Result<HashSet<String>> {
    let mut families = referenced_segments
        .iter()
        .filter_map(|segment| segment_family(segment).map(ToOwned::to_owned))
        .collect::<HashSet<_>>();

    for playlist in media_playlists {
        // Populated playlists establish ownership through their actual resources,
        // including video renditions whose names happen to end in `_vtt`.
        if !playlist_segment_entries(playlist)?.is_empty() {
            continue;
        }

        let stem = Path::new(playlist)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .with_context(|| format!("HLS playlist path has no valid stem: {playlist}"))?;
        if let Some(vtt_stem) = stem.strip_suffix("_vtt") {
            // FFmpeg writes `stream_vtt.m3u8` but names its WebVTT segments
            // `stream0.vtt`, without the separator used for MPEG-TS segments.
            families.insert(vtt_stem.to_string());
        } else {
            families.insert(format!("{stem}_"));
        }
    }

    Ok(families)
}

fn segment_family(file_name: &str) -> Option<&str> {
    let path = Path::new(file_name);
    let extension = path.extension()?.to_str()?;

    if !matches!(extension, "ts" | "vtt" | "m4s" | "mp4") {
        return None;
    }

    let stem = if extension == "mp4" {
        file_name.strip_suffix(".init.mp4")?
    } else {
        path.file_stem()?.to_str()?
    };
    let number_start = stem
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    (number_start < stem.len() && (extension != "mp4" || stem[..number_start].ends_with('_')))
        .then_some(&stem[..number_start])
}

fn common_playlist_parent(media_playlists: &[String]) -> Option<PathBuf> {
    let first = media_playlists.first()?;
    Some(
        Path::new(first)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_owned(),
    )
}

fn is_hls_segment(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "ts" | "vtt" | "m4s" | "mp4"))
}

fn remove_playlist(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub(super) fn master_playlist_path(path: &str) -> PathBuf {
    Path::new(path).with_file_name("master.m3u8")
}

fn media_sequence_start_number(paths: &[String]) -> Result<Option<u64>> {
    for path in paths {
        let playlist = match fs::read_to_string(path) {
            Ok(playlist) => playlist,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read HLS playlist {path}"));
            }
        };

        if let Some(sequence) = media_sequence(&playlist) {
            return Ok(Some(sequence));
        }
    }

    Ok(None)
}

fn media_sequence(playlist: &str) -> Option<u64> {
    playlist.lines().find_map(|line| {
        line.trim()
            .strip_prefix("#EXT-X-MEDIA-SEQUENCE:")
            .and_then(|value| value.trim().parse().ok())
    })
}

fn playlist_uri(line: &str) -> Option<&str> {
    let entry = line.trim();

    if entry.is_empty() || entry.starts_with('#') {
        return None;
    }

    Some(
        entry
            .split_once('?')
            .map_or(entry, |(entry, _)| entry)
            .split_once('#')
            .map_or(entry, |(entry, _)| entry),
    )
}

pub(super) fn segment_pattern(path: &str) -> String {
    Path::new(path)
        .with_file_name("%v_%d.ts")
        .to_string_lossy()
        .into_owned()
}

pub(super) fn standalone_segment_pattern(path: &str) -> String {
    let path = Path::new(path);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("stream");
    path.with_file_name(format!("{stem}_%d.ts"))
        .to_string_lossy()
        .into_owned()
}

pub(super) fn var_stream_map(variants: &[HlsVariant], subtitle: Option<&HlsSubtitle>) -> String {
    variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            if let Some(subtitle) = subtitle.filter(|_| index == 0) {
                let default = if subtitle.default { "YES" } else { "NO" };
                let subtitle_name = if ffmpeg_capabilities().features.hls_subtitle_name {
                    format!(",sname:{}", subtitle.name)
                } else {
                    String::new()
                };
                format!(
                    "v:{index},a:{index},s:0,sgroup:subs,name:{}{subtitle_name},language:{},default:{default}",
                    variant.name, subtitle.language
                )
            } else {
                format!("v:{index},a:{index},name:{}", variant.name)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::{env, process};

    use super::*;

    fn variant(name: &str) -> HlsVariant {
        HlsVariant {
            name: name.to_string(),
            width: 1_280,
            height: 720,
            video_bitrate: 3_000_000,
            audio_bitrate: 128_000,
        }
    }

    fn subtitle() -> HlsSubtitle {
        HlsSubtitle {
            name: "Deutsch".to_string(),
            language: "de-DE".to_string(),
            default: false,
        }
    }

    #[test]
    fn playlist_path_is_unchanged_without_variants() {
        assert_eq!(
            playlist_path("live/index.m3u8", &[]).unwrap(),
            "live/index.m3u8"
        );
    }

    #[test]
    fn playlist_path_uses_variant_name_as_file_name() {
        assert_eq!(
            playlist_path("live/index.m3u8", &[variant("high")]).unwrap(),
            "live/%v.m3u8"
        );
    }

    #[test]
    fn resolved_variant_path_uses_variant_name() {
        assert_eq!(
            resolved_variant_playlist_path("live/index.m3u8", "high").unwrap(),
            "live/high.m3u8"
        );
    }

    #[test]
    fn playlist_path_rejects_missing_file_name() {
        assert!(playlist_path("/", &[variant("high")]).is_err());
    }

    #[test]
    fn validate_variants_rejects_duplicate_names() {
        assert!(validate_variants(&[variant("high"), variant("high")]).is_err());
    }

    #[test]
    fn validate_variants_accepts_unique_names() {
        assert!(validate_variants(&[variant("high"), variant("low")]).is_ok());
    }

    #[test]
    fn segment_pattern_prefixes_file_name() {
        assert_eq!(segment_pattern("live/index.m3u8"), "live/%v_%d.ts");
    }

    #[test]
    fn standalone_segment_pattern_uses_playlist_stem() {
        assert_eq!(
            standalone_segment_pattern("live/stream.m3u8"),
            "live/stream_%d.ts"
        );
    }

    #[test]
    fn resume_uses_playlist_media_sequence_as_start_number() {
        let dir = env::temp_dir().join(format!("hls_sequence_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream.m3u8");
        fs::write(
            &playlist,
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:42\n#EXTINF:1.0,\nstream_42.ts\n",
        )
        .unwrap();

        assert_eq!(
            media_sequence_start_number(&[playlist.to_string_lossy().into_owned()]).unwrap(),
            Some(42)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prune_unreferenced_segments_keeps_playlist_entries() {
        let dir = env::temp_dir().join(format!("hls_prune_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream.m3u8");
        let subtitle_playlist = dir.join("stream_vtt.m3u8");
        fs::write(&playlist, "#EXTM3U\n#EXTINF:1.0,\nstream_1.ts\n").unwrap();
        fs::write(&subtitle_playlist, "#EXTM3U\n#EXTINF:1.0,\nstream1.vtt\n").unwrap();
        fs::write(dir.join("stream_1.ts"), b"ts").unwrap();
        fs::write(dir.join("stream1.vtt"), b"vtt").unwrap();
        fs::write(dir.join("stream_0.ts"), b"old").unwrap();
        fs::write(dir.join("stream0.vtt"), b"old").unwrap();
        fs::write(dir.join("other_0.ts"), b"other output").unwrap();

        prune_unreferenced_segments(&[
            playlist.to_string_lossy().into_owned(),
            subtitle_playlist.to_string_lossy().into_owned(),
        ])
        .unwrap();

        assert!(dir.join("stream_1.ts").exists());
        assert!(dir.join("stream1.vtt").exists());
        assert!(!dir.join("stream_0.ts").exists());
        assert!(!dir.join("stream0.vtt").exists());
        assert!(dir.join("other_0.ts").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_subtitle_playlist_still_prunes_its_vtt_segments() {
        let dir = env::temp_dir().join(format!("hls_vtt_prune_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream_vtt.m3u8");
        fs::write(&playlist, "#EXTM3U\n").unwrap();
        fs::write(dir.join("stream0.vtt"), b"orphaned").unwrap();
        fs::write(dir.join("other0.vtt"), b"other output").unwrap();

        prune_unreferenced_segments(&[playlist.to_string_lossy().into_owned()]).unwrap();

        assert!(!dir.join("stream0.vtt").exists());
        assert!(dir.join("other0.vtt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cleanup_playlist_paths_include_master_children_only() {
        let dir = env::temp_dir().join(format!("hls_master_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let master = dir.join("master.m3u8");
        let stream = dir.join("stream.m3u8");
        let subtitles = dir.join("stream_vtt.m3u8");
        let unrelated = dir.join("other.m3u8");
        fs::write(
            &master,
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,URI=\"stream_vtt.m3u8\"\nstream.m3u8\n",
        )
        .unwrap();
        fs::write(&unrelated, "#EXTM3U\n").unwrap();

        let paths = cleanup_playlist_paths(
            &[stream.to_string_lossy().into_owned()],
            Some(&master.to_string_lossy()),
        )
        .unwrap()
        .into_iter()
        .collect::<HashSet<_>>();

        assert!(paths.contains(&master.to_string_lossy().into_owned()));
        assert!(paths.contains(&stream.to_string_lossy().into_owned()));
        assert!(paths.contains(&subtitles.to_string_lossy().into_owned()));
        assert!(!paths.contains(&unrelated.to_string_lossy().into_owned()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stale_playlist_keeps_old_media_until_replacement() {
        let dir = env::temp_dir().join(format!("hls_stale_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream.m3u8");
        fs::write(&playlist, "#EXTM3U\n#EXTINF:1.0,\nstream_1.ts\n").unwrap();
        fs::write(dir.join("stream_0.ts"), b"unreferenced").unwrap();
        fs::write(dir.join("stream_1.ts"), b"referenced").unwrap();
        fs::write(dir.join("other_0.ts"), b"other output").unwrap();
        File::open(&playlist)
            .unwrap()
            .set_modified(SystemTime::now() - HLS_RESUME_MAX_AGE - Duration::from_secs(1))
            .unwrap();

        let paths = [playlist.to_string_lossy().into_owned()];
        let session = HlsSession::acquire(&paths, None, false).unwrap();

        assert!(session.requires_replacement(None).unwrap());
        let mut replacement = session.replacement(false).unwrap();

        assert!(playlist.exists());
        assert!(dir.join("stream_1.ts").exists());
        fs::write(&playlist, "#EXTM3U\n").unwrap();
        let now = SystemTime::now();
        replacement.protect(now).unwrap();
        assert!(!replacement.retire(now).unwrap());
        for path in &replacement.published_media {
            assert!(path.is_file());
        }
        assert!(replacement.retire(now + Duration::from_secs(10)).unwrap());

        assert!(playlist.exists());
        assert!(!dir.join("stream_0.ts").exists());
        assert!(!dir.join("stream_1.ts").exists());
        assert!(dir.join("other_0.ts").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn var_stream_map_without_subtitles() {
        let map = var_stream_map(&[variant("high"), variant("low")], None);
        assert_eq!(map, "v:0,a:0,name:high v:1,a:1,name:low");
    }

    #[test]
    fn var_stream_map_links_subtitles_to_first_variant_only() {
        let subtitle = subtitle();
        let map = var_stream_map(&[variant("high"), variant("low")], Some(&subtitle));
        let expected = if ffmpeg_capabilities().features.hls_subtitle_name {
            "v:0,a:0,s:0,sgroup:subs,name:high,sname:Deutsch,language:de-DE,default:NO v:1,a:1,name:low"
        } else {
            "v:0,a:0,s:0,sgroup:subs,name:high,language:de-DE,default:NO v:1,a:1,name:low"
        };
        assert_eq!(map, expected);
    }
}

#[cfg(test)]
mod packager_switch_tests {
    use super::*;

    #[test]
    fn native_container_switch_clears_fragments_and_owned_initializations() {
        let directory =
            std::env::temp_dir().join(format!("hls-container-switch-{}", std::process::id()));
        fs::remove_dir_all(&directory).ok();
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("stream.m3u8");
        fs::write(&path, "#EXTM3U\n# ffplayout-packager: native\n#EXT-X-MAP:URI=\"stream_1.init.mp4\"\n#EXTINF:1.0,\nstream_1.m4s\n").unwrap();
        let owned = [
            "stream_0.m4s",
            "stream_1.m4s",
            "stream_1.init.mp4",
            "stream_2.init.mp4",
        ];
        let unrelated = ["stream_3.mp4", "foreign_1.m4s", "foreign_1.init.mp4"];
        for name in owned.into_iter().chain(unrelated) {
            fs::write(directory.join(name), "media").unwrap();
        }

        let session =
            HlsSession::acquire(&[path.to_string_lossy().into_owned()], None, false).unwrap();
        assert!(session.requires_replacement(Some(false)).unwrap());
        let mut replacement = session.replacement(false).unwrap();
        assert!(path.exists());
        assert!(owned.iter().all(|name| directory.join(name).exists()));
        fs::write(&path, "#EXTM3U\n").unwrap();
        let now = SystemTime::now();
        replacement.protect(now).unwrap();
        assert!(!replacement.retire(now).unwrap());
        for path in &replacement.published_media {
            assert!(path.is_file());
        }
        assert!(replacement.retire(now + Duration::from_secs(10)).unwrap());
        assert!(path.exists());
        assert!(owned.iter().all(|name| !directory.join(name).exists()));
        assert!(unrelated.iter().all(|name| directory.join(name).exists()));

        drop(session);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_grace_survives_restart_with_different_segment_names() {
        let directory =
            std::env::temp_dir().join(format!("hls-replacement-grace-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let playlist = directory.join("stream.m3u8");
        let paths = [playlist.to_string_lossy().into_owned()];
        let init = directory.join("stream_1.init.mp4");
        let fragment = directory.join("stream_1.m4s");
        fs::write(&init, b"init").unwrap();
        fs::write(&fragment, b"fragment").unwrap();
        fs::write(
            &playlist,
            "#EXTM3U\n#EXT-X-MAP:URI=\"stream_1.init.mp4\"\n#EXTINF:4,\nstream_1.m4s\n",
        )
        .unwrap();

        let now = SystemTime::now();
        {
            let session = HlsSession::acquire(&paths, None, false).unwrap();
            let mut previous = session.replacement(false).unwrap();
            previous.protect(now).unwrap();
            fs::write(&playlist, "#EXTM3U\n#EXTINF:4,\n.hls-new-stream_2.ts\n").unwrap();
            fs::write(directory.join(".hls-new-stream_2.ts"), b"new").unwrap();
            assert!(!previous.retire(now).unwrap());
        }
        let mut resumed = HlsSession::acquire(&paths, None, false).unwrap();
        resumed.resume_start_number().unwrap();
        assert_eq!(resumed.pending_media.len(), 2);
        retire_media(&mut resumed.pending_media, now + Duration::from_secs(7)).unwrap();
        assert_eq!(fs::read(&init).unwrap(), b"init");
        assert_eq!(fs::read(&fragment).unwrap(), b"fragment");
        retire_media(&mut resumed.pending_media, now + Duration::from_secs(9)).unwrap();
        assert!(resumed.pending_media.is_empty());
        assert!(!init.exists());
        assert!(!fragment.exists());
        assert_eq!(
            fs::read(directory.join(".hls-new-stream_2.ts")).unwrap(),
            b"new"
        );
        drop(resumed);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn backend_switch_removes_inactive_subtitles_and_retained_media() {
        let directory =
            std::env::temp_dir().join(format!("hls-backend-inactive-{}", std::process::id()));

        fs::remove_dir_all(&directory).ok();
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("stream.m3u8"),
            "#EXTM3U\n# ffplayout-packager: native\n#EXTINF:1.0,\nstream_1.ts\n",
        )
        .unwrap();
        fs::write(
            directory.join("stream_vtt.m3u8"),
            "#EXTM3U\n# ffplayout-packager: native\n#EXTINF:1.0,\nstream0.vtt\n",
        )
        .unwrap();

        for name in [
            "stream_0.ts",
            "stream_1.ts",
            "stream0.vtt",
            "stream9.vtt",
            "foreign_0.ts",
        ] {
            fs::write(directory.join(name), "media").unwrap();
        }

        let path = directory.join("stream.m3u8").to_string_lossy().into_owned();
        let session = HlsSession::acquire(&[path], None, false).unwrap();

        assert!(session.requires_replacement(None).unwrap());
        let mut replacement = session.replacement(false).unwrap();
        assert!(directory.join("stream_vtt.m3u8").exists());
        assert!(directory.join("stream_1.ts").exists());
        let now = SystemTime::now();
        replacement.protect(now).unwrap();
        assert!(!replacement.retire(now).unwrap());
        for path in &replacement.published_media {
            assert!(path.is_file());
        }
        assert!(replacement.retire(now + Duration::from_secs(10)).unwrap());

        for name in [
            "stream_vtt.m3u8",
            "stream_0.ts",
            "stream_1.ts",
            "stream0.vtt",
            "stream9.vtt",
        ] {
            assert!(!directory.join(name).exists(), "{name} was not cleared");
        }

        assert!(directory.join("foreign_0.ts").exists());
        drop(session);
        fs::remove_dir_all(directory).unwrap();
    }
}
