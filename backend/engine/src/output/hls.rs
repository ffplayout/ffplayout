use std::{collections::HashSet, fs, io::ErrorKind, path::Path, ptr};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next as ffmpeg;

use crate::utils::config::{HlsSubtitle, HlsVariant};

pub(super) fn playlist_path(path: &str, variants: &[HlsVariant]) -> Result<String> {
    if variants.is_empty() {
        return Ok(path.to_string());
    }

    let path = Path::new(path);
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .context("HLS playlist path must include a file name")?;
    Ok(path
        .with_file_name(format!("%v_{file_name}"))
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
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .context("HLS playlist path must include a file name")?;
    Ok(path
        .with_file_name(format!("{variant_name}_{file_name}"))
        .to_string_lossy()
        .into_owned())
}

pub(super) fn validate_variants(variants: &[HlsVariant]) -> Result<()> {
    let mut names = HashSet::new();
    for variant in variants {
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

pub(super) fn segment_pattern(path: &str) -> String {
    Path::new(path)
        .with_file_name("%v_%d.ts")
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
                format!(
                    "v:{index},a:{index},s:0,sgroup:subs,name:{},sname:{},language:{},default:{default}",
                    variant.name, subtitle.name, subtitle.language
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
    fn playlist_path_prefixes_file_name_with_variants() {
        assert_eq!(
            playlist_path("live/index.m3u8", &[variant("high")]).unwrap(),
            "live/%v_index.m3u8"
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
    fn var_stream_map_without_subtitles() {
        let map = var_stream_map(&[variant("high"), variant("low")], None);
        assert_eq!(map, "v:0,a:0,name:high v:1,a:1,name:low");
    }

    #[test]
    fn var_stream_map_links_subtitles_to_first_variant_only() {
        let subtitle = subtitle();
        let map = var_stream_map(&[variant("high"), variant("low")], Some(&subtitle));
        assert_eq!(
            map,
            "v:0,a:0,s:0,sgroup:subs,name:high,sname:Deutsch,language:de-DE,default:NO v:1,a:1,name:low"
        );
    }
}
