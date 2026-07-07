use std::{
    collections::HashSet,
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

pub(super) fn close_preopened_output(
    octx: &mut ffmpeg::format::context::Output,
    path: &str,
) -> Result<()> {
    unsafe {
        let context = octx.as_mut_ptr();
        if !(*context).pb.is_null() {
            let result = ffmpeg::ffi::avio_close((*context).pb);
            (*context).pb = ptr::null_mut();
            if result < 0 {
                return Err(ffmpeg::Error::from(result).into());
            }
        }
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove HLS placeholder {path}"))
        }
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

    if latest
        .elapsed()
        .unwrap_or(Duration::ZERO)
        .gt(&HLS_RESUME_MAX_AGE)
    {
        remove_stale_playlists(&cleanup_playlists, master_playlist)?;
        return Ok(None);
    }

    next_segment_start_number(media_playlists)
}

fn cleanup_playlist_paths(
    media_playlists: &[String],
    master_playlist: Option<&str>,
) -> Result<Vec<String>> {
    let mut paths = media_playlists.iter().cloned().collect::<HashSet<_>>();
    if let Some(master_playlist) = master_playlist {
        paths.insert(master_playlist.to_string());
    }
    if let Some(parent) = common_playlist_parent(media_playlists) {
        for entry in fs::read_dir(&parent)
            .with_context(|| format!("failed to read HLS directory {}", parent.display()))?
        {
            let entry =
                entry.with_context(|| format!("failed to read entry in {}", parent.display()))?;
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension == "m3u8")
            {
                paths.insert(path.to_string_lossy().into_owned());
            }
        }
    }

    Ok(paths.into_iter().collect())
}

fn prune_unreferenced_segments(media_playlists: &[String]) -> Result<()> {
    let referenced_segments = referenced_segments(media_playlists)?;
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
        if !referenced_segments.contains(file_name) {
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

pub(super) fn next_segment_start_number(paths: &[String]) -> Result<Option<u64>> {
    let mut last_segment = None;
    for path in paths {
        let playlist = match fs::read_to_string(path) {
            Ok(playlist) => playlist,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read HLS playlist {path}"));
            }
        };
        last_segment = playlist
            .lines()
            .filter_map(segment_number_from_playlist_entry)
            .max()
            .or(last_segment);
    }

    Ok(last_segment.map(|number| number + 1))
}

fn segment_number_from_playlist_entry(line: &str) -> Option<u64> {
    let entry = playlist_uri(line)?;
    let file_name = entry.rsplit('/').next()?;
    let stem = Path::new(file_name).file_stem()?.to_str()?;
    let end = stem
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_ascii_digit())?
        .0
        + 1;
    if end != stem.len() {
        return None;
    }
    let start = stem[..end]
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(index, ch)| index + ch.len_utf8());

    stem[start..end].parse().ok()
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
    fn next_segment_start_number_uses_highest_playlist_segment() {
        let dir = std::env::temp_dir().join(format!("hls_number_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("stream.m3u8");
        let second = dir.join("low.m3u8");
        fs::write(
            &first,
            "#EXTM3U\n#EXTINF:1.0,\nstream_7.ts\n#EXTINF:1.0,\nstream_8.ts?token=1\n",
        )
        .unwrap();
        fs::write(&second, "#EXTM3U\n#EXTINF:1.0,\nlow_12.ts\n").unwrap();

        assert_eq!(
            next_segment_start_number(&[
                first.to_string_lossy().into_owned(),
                second.to_string_lossy().into_owned(),
            ])
            .unwrap(),
            Some(13)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prune_unreferenced_segments_keeps_playlist_entries() {
        let dir = std::env::temp_dir().join(format!("hls_prune_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("stream.m3u8");
        fs::write(
            &playlist,
            "#EXTM3U\n#EXTINF:1.0,\nstream_1.ts\n#EXTINF:1.0,\nsubs_1.vtt\n",
        )
        .unwrap();
        fs::write(dir.join("stream_1.ts"), b"ts").unwrap();
        fs::write(dir.join("subs_1.vtt"), b"vtt").unwrap();
        fs::write(dir.join("stream_0.ts"), b"old").unwrap();
        fs::write(dir.join("subs_0.vtt"), b"old").unwrap();

        prune_unreferenced_segments(&[playlist.to_string_lossy().into_owned()]).unwrap();

        assert!(dir.join("stream_1.ts").exists());
        assert!(dir.join("subs_1.vtt").exists());
        assert!(!dir.join("stream_0.ts").exists());
        assert!(!dir.join("subs_0.vtt").exists());
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
