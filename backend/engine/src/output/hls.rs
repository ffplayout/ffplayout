use std::{
    collections::HashSet,
    ffi::CString,
    fs,
    io::ErrorKind,
    path::Path,
    ptr,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, anyhow};
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

pub(super) fn remove_master_playlist(path: &str) -> Result<()> {
    let master = master_playlist_path(path);
    remove_playlist(&master)
}

pub(super) fn prepare_resume_start_number(
    media_playlists: &[String],
    master_playlist: Option<&str>,
) -> Result<Option<u64>> {
    prepare_resume_start_number_at(media_playlists, master_playlist, SystemTime::now())
}

fn prepare_resume_start_number_at(
    media_playlists: &[String],
    master_playlist: Option<&str>,
    now: SystemTime,
) -> Result<Option<u64>> {
    let cleanup_playlists = cleanup_playlist_paths(media_playlists, master_playlist)?;
    prune_unreferenced_segments(&cleanup_playlists)?;

    let mut latest = None;
    for path in &cleanup_playlists {
        if let Some(modified) = fresh_playlist_modified_time(path)? {
            latest = latest.max(Some(modified));
        }
    }

    let Some(latest) = latest else {
        return Ok(None);
    };

    if now
        .duration_since(latest)
        .unwrap_or(Duration::ZERO)
        .gt(&HLS_RESUME_MAX_AGE)
    {
        remove_stale_playlists(&cleanup_playlists, master_playlist)?;
        return Ok(None);
    }

    media_sequence_start_number(media_playlists)
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
            remove_playlist(&path)?;
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

fn remove_stale_playlists(media_playlists: &[String], master_playlist: Option<&str>) -> Result<()> {
    for segment in referenced_segment_paths(media_playlists)? {
        remove_playlist(&segment)?;
    }
    for playlist in media_playlists {
        remove_playlist(Path::new(playlist))
            .with_context(|| format!("failed to remove stale HLS playlist {playlist}"))?;
    }
    if let Some(master_playlist) = master_playlist {
        remove_playlist(Path::new(master_playlist))
            .with_context(|| format!("failed to remove stale HLS master {master_playlist}"))?;
    }
    Ok(())
}

fn referenced_segment_paths(media_playlists: &[String]) -> Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    for playlist in media_playlists {
        let Some(parent) = Path::new(playlist).parent() else {
            continue;
        };
        for segment in playlist_segment_entries(playlist)? {
            paths.push(parent.join(segment));
        }
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
        .filter_map(playlist_uri)
        .map(ToOwned::to_owned)
        .collect())
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
            Path::new(entry)
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
    if extension != "ts" && extension != "vtt" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let number_start = stem
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    (number_start < stem.len()).then_some(&stem[..number_start])
}

fn common_playlist_parent(media_playlists: &[String]) -> Option<std::path::PathBuf> {
    let first = media_playlists.first()?;
    Path::new(first).parent().map(Path::to_path_buf)
}

fn is_hls_segment(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "ts" || extension == "vtt")
}

fn remove_playlist(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub(super) fn master_playlist_path(path: &str) -> std::path::PathBuf {
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
        if let Some(sequence) = playlist.lines().find_map(|line| {
            line.trim()
                .strip_prefix("#EXT-X-MEDIA-SEQUENCE:")
                .and_then(|value| value.trim().parse().ok())
        }) {
            return Ok(Some(sequence));
        }
    }

    Ok(None)
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
        let dir = std::env::temp_dir().join(format!("hls_sequence_test_{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("hls_prune_test_{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("hls_vtt_prune_test_{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("hls_master_test_{}", std::process::id()));
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
    fn stale_playlist_starts_clean_without_touching_other_output() {
        let dir = std::env::temp_dir().join(format!("hls_stale_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream.m3u8");
        fs::write(&playlist, "#EXTM3U\n#EXTINF:1.0,\nstream_1.ts\n").unwrap();
        fs::write(dir.join("stream_0.ts"), b"unreferenced").unwrap();
        fs::write(dir.join("stream_1.ts"), b"referenced").unwrap();
        fs::write(dir.join("other_0.ts"), b"other output").unwrap();
        let now = SystemTime::now() + HLS_RESUME_MAX_AGE + Duration::from_secs(1);

        let start_number =
            prepare_resume_start_number_at(&[playlist.to_string_lossy().into_owned()], None, now)
                .unwrap();

        assert_eq!(start_number, None);
        assert!(!playlist.exists());
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
