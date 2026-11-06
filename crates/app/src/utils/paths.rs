use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use uuid::Uuid;

use crate::{db::models::Channel, utils::errors::ServiceError};

/// Owns only directories created by this validation. Failed saves remove them
/// in reverse order, and only while empty; existing directories are untouched.
#[derive(Default)]
pub struct WritableDirectories {
    created: Vec<PathBuf>,
}

impl WritableDirectories {
    pub fn commit(mut self) {
        self.created.clear();
    }

    fn prepare(&mut self, path: &Path) -> io::Result<()> {
        let mut missing = Vec::new();
        let mut ancestor = path;

        while !ancestor.try_exists()? {
            missing.push(ancestor.to_path_buf());
            ancestor = ancestor
                .parent()
                .ok_or_else(|| io::Error::other("no existing parent"))?;
        }

        for directory in missing.into_iter().rev() {
            match fs::create_dir(&directory) {
                Ok(()) => self.created.push(directory),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }

        let probe = path.join(format!(".ffplayout-write-test-{}", Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        let result = file
            .write_all(b"ffplayout directory check\n")
            .and_then(|()| file.sync_all());

        drop(file);

        let cleanup = fs::remove_file(&probe);

        result.and(cleanup)
    }
}

impl Drop for WritableDirectories {
    fn drop(&mut self) {
        for directory in self.created.iter().rev() {
            // Never recursively remove data another request may have created.
            let _ = fs::remove_dir(directory);
        }
    }
}

/// Probe with the service's actual credentials, including ACL and mount policy.
/// All filesystem work runs outside the async executor's worker threads.
pub async fn prepare_writable_directories(
    paths: Vec<(&'static str, PathBuf)>,
) -> Result<WritableDirectories, ServiceError> {
    tokio::task::spawn_blocking(move || {
        // Validate every path before creating anything.
        for (name, path) in &paths {
            if path.as_os_str().to_string_lossy().trim() != path.as_os_str().to_string_lossy() {
                return Err(ServiceError::BadRequest(format!(
                    "{name} path must not have leading or trailing whitespace"
                )));
            }

            validate_directory_path(name, &path.to_string_lossy())
                .map_err(ServiceError::BadRequest)?;
        }

        let mut directories = WritableDirectories::default();
        let mut checked = HashSet::new();

        for (name, path) in paths {
            if !checked.insert(path.clone()) {
                continue;
            }

            directories.prepare(&path).map_err(|error| {
                ServiceError::BadRequest(format!(
                    "{name} directory {} is not writable by the ffplayout service: {error}",
                    path.display()
                ))
            })?;
        }

        Ok(directories)
    })
    .await?
}

pub async fn prepare_channel_directories(
    channel: &Channel,
    logs: &str,
) -> Result<WritableDirectories, ServiceError> {
    prepare_writable_directories(vec![
        ("Logging", PathBuf::from(logs)),
        ("Public", PathBuf::from(&channel.public)),
        ("Playlist", PathBuf::from(&channel.playlists)),
        ("Storage", PathBuf::from(&channel.storage)),
    ])
    .await
}

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
    // Compare against the filesystem's real system roots as well. For example,
    // macOS resolves /etc and /var to /private/etc and /private/var.
    let system_paths = PROTECTED_SYSTEM_PATHS
        .iter()
        .map(|system| {
            let system = Path::new(system);
            resolve_existing_ancestor(system).map(|resolved| (system, resolved))
        })
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| format!("{name} system paths could not be resolved: {error}"))?;
    let is_application_path = APPLICATION_SYSTEM_PATHS.iter().any(|allowed| {
        resolved.starts_with(allowed)
            || system_paths.iter().any(|(system, canonical)| {
                // Translate only the trusted system root, not the complete
                // allowed directory: an application-directory symlink must
                // not turn its target into another allowed system path.
                Path::new(allowed)
                    .strip_prefix(system)
                    .is_ok_and(|suffix| resolved.starts_with(canonical.join(suffix)))
            })
    });
    let is_protected_system_path = system_paths
        .iter()
        .any(|(system, canonical)| resolved.starts_with(system) || resolved.starts_with(canonical));
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
    #[cfg(unix)]
    use std::{fs, os::unix::fs::symlink};

    use super::validate_directory_path;

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

    #[test]
    fn validates_canonical_system_and_application_paths() {
        let system = super::resolve_existing_ancestor(std::path::Path::new("/etc")).unwrap();
        assert!(validate_directory_path("Recording", system.to_str().unwrap()).is_err());

        let application = super::resolve_existing_ancestor(std::path::Path::new(
            "/var/lib/ffplayout/recordings/1",
        ))
        .unwrap();
        assert!(validate_directory_path("Recording", application.to_str().unwrap()).is_ok());
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

#[cfg(test)]
mod writable_tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{env, fs, path::PathBuf};

    use uuid::Uuid;

    use super::prepare_writable_directories;

    fn root() -> PathBuf {
        env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("write-check-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn probes_leave_no_files_and_failed_saves_remove_only_new_empty_directories() {
        let root = root();

        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("existing"), b"keep").unwrap();

        let new = root.join("new/nested");
        let prepared = prepare_writable_directories(vec![
            ("Storage", root.clone()),
            ("Playlist", new.clone()),
        ])
        .await
        .unwrap();

        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert_eq!(fs::read_dir(&new).unwrap().count(), 0);

        drop(prepared);

        assert!(!root.join("new").exists());
        assert_eq!(fs::read(root.join("existing")).unwrap(), b"keep");

        let prepared = prepare_writable_directories(vec![("Storage", new.clone())])
            .await
            .unwrap();

        prepared.commit();

        assert!(new.is_dir());

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failure_in_a_later_directory_rolls_back_earlier_creation() {
        let root = root();

        fs::create_dir_all(&root).unwrap();

        let file = root.join("file");

        fs::write(&file, b"keep").unwrap();

        let result = prepare_writable_directories(vec![
            ("Storage", root.join("new/nested")),
            ("Playlist", file.clone()),
        ])
        .await;
        let error = result.err().unwrap().to_string();

        assert!(error.contains("Playlist") && error.contains("not writable"));
        assert!(!root.join("new").exists());

        assert_eq!(fs::read(&file).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rollback_keeps_new_directories_that_have_received_data() {
        let root = root();
        let new = root.join("nested");
        let prepared = prepare_writable_directories(vec![("Storage", new.clone())])
            .await
            .unwrap();

        fs::write(new.join("media"), b"keep").unwrap();

        drop(prepared);

        assert_eq!(fs::read(new.join("media")).unwrap(), b"keep");

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn existing_read_only_directory_is_rejected_without_changing_its_permissions() {
        let root = root();

        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();

        let result = prepare_writable_directories(vec![("Storage", root.clone())]).await;
        let mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;

        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(&root).unwrap();

        // A root service may legitimately write here despite mode bits.
        if env::var("USER").as_deref() != Ok("root") {
            assert!(result.is_err());
        }

        assert_eq!(mode, 0o555);
    }
}
