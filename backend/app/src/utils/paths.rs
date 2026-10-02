use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Directories that must never be used directly as ffplayout data directories.
const PROTECTED_SYSTEM_PATHS: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin", "/sys",
    "/tmp", "/usr", "/var",
];

/// ffplayout's own directories, which are allowed even though they live under
/// a protected system path (e.g. `/var`).
const APPLICATION_SYSTEM_PATHS: &[&str] = &[
    "/usr/share/ffplayout",
    "/var/lib/ffplayout",
    "/var/log/ffplayout",
    "/var/www",
];

/// Validates that `value` is a safe, absolute directory path: non-empty, absolute,
/// free of `.`/`..` components, and (when it falls under a protected system
/// directory) only allowed when it also lives under one of ffplayout's own
/// application directories.
pub fn validate_directory_path(name: &str, value: &str) -> Result<(), String> {
    let path = Path::new(value.trim());
    if value.trim().is_empty() || !path.is_absolute() || path.parent().is_none() {
        return Err(format!("{name} path must be an absolute directory"));
    }

    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(format!("{name} path must not contain relative components"));
    }

    validate_resolved_path(name, path)?;

    Ok(())
}

fn validate_resolved_path(name: &str, path: &Path) -> Result<(), String> {
    let resolved = resolve_existing_ancestor(path)
        .map_err(|error| format!("{name} path could not be resolved: {error}"))?;
    let is_application_path = APPLICATION_SYSTEM_PATHS
        .iter()
        .any(|allowed| resolved.starts_with(allowed));
    let is_protected_system_path = PROTECTED_SYSTEM_PATHS
        .iter()
        .any(|protected| resolved.starts_with(protected));
    if is_protected_system_path && !is_application_path {
        return Err(format!("{name} path must not point to a system directory"));
    }

    Ok(())
}

pub(crate) fn resolve_existing_ancestor(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();

    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name() else {
                    return Err(error);
                };
                missing.push(name.to_os_string());
                existing = existing.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }

    let mut resolved = fs::canonicalize(existing)?;

    for component in missing.iter().rev() {
        resolved.push(component);
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::validate_directory_path;
    #[cfg(unix)]
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn rejects_empty_and_relative_paths() {
        assert!(validate_directory_path("Recording", "").is_err());
        assert!(validate_directory_path("Recording", "recordings").is_err());
        assert!(validate_directory_path("Recording", "/var/lib/ffplayout/../etc").is_err());
    }

    #[test]
    fn rejects_protected_system_paths() {
        assert!(validate_directory_path("Recording", "/etc").is_err());
        assert!(validate_directory_path("Recording", "/var/log").is_err());
    }

    #[test]
    fn accepts_application_paths() {
        assert!(validate_directory_path("Recording", "/var/lib/ffplayout/recordings/1").is_ok());
        assert!(validate_directory_path("Recording", "/srv/recordings").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_paths_reaching_system_directories_through_symlinks() {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("ffplayout-path-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let link = root.join("recordings");
        let _ = fs::remove_file(&link);
        symlink("/etc", &link).unwrap();

        assert!(validate_directory_path("Recording", link.to_str().unwrap()).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
