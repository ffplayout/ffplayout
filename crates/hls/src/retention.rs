//! Expiry markers survive writer restarts without changing the served media.
use std::{
    fs,
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const SUFFIX: &str = ".hls-retain";

fn marker(path: &Path) -> PathBuf {
    path.with_added_extension("hls-retain")
}

pub fn marked_resource(path: &Path) -> Option<PathBuf> {
    Some(path.with_file_name(path.file_name()?.to_str()?.strip_suffix(SUFFIX)?))
}

/// Keep a cached playlist playable for its window plus its longest segment.
pub fn grace(durations: impl Iterator<Item = f64>) -> Duration {
    let (window, longest) = durations
        .filter(|s| s.is_finite() && *s > 0.0)
        .fold((0.0, 0.0_f64), |(sum, max), duration| {
            (sum + duration, max.max(duration))
        });
    Duration::from_secs((window + longest).ceil().max(1.0) as u64)
}

pub fn protect(path: &Path, until: SystemTime, owner: &Path) -> io::Result<()> {
    let marker = marker(path);
    let deadline = match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_file() => metadata.modified()?.max(until),
        Ok(_) => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid HLS retention marker",
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => until,
        Err(error) => return Err(error),
    };
    let name = owner
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ErrorKind::InvalidInput)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".hls-retention-");

    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};

        builder.permissions(Permissions::from_mode(0o666));
    }
    let mut file = builder.tempfile_in(marker.parent().ok_or(ErrorKind::InvalidInput)?)?;
    file.write_all(name.as_bytes())?;
    file.as_file().set_modified(deadline)?;
    file.as_file().sync_all()?;
    file.persist(marker).map_err(|error| error.error)?;
    Ok(())
}

/// Returns false while an old playlist can still refer to this resource.
pub fn remove(path: &Path, now: SystemTime) -> io::Result<bool> {
    let marker = marker(path);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid HLS retention marker",
            ));
        }
        Ok(metadata) if metadata.modified()? > now => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return unlink(path).map(|()| true),
        Err(error) => return Err(error),
    }
    unlink(path)?;
    unlink(&marker)?;
    Ok(true)
}

fn unlink(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
