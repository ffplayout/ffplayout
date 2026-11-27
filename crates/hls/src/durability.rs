use std::{io, path::Path};

/// Synchronize a published file's directory entry when the filesystem supports it.
/// File data must be synchronized separately before publication.
pub fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::{fs::File, sync::Once};

        static WARN_UNSUPPORTED: Once = Once::new();
        let file = File::open(directory)?;

        loop {
            match file.sync_all() {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if unsupported_directory_sync(&error) => {
                    WARN_UNSUPPORTED.call_once(|| {
                        log::warn!(
                            "HLS directory sync is unsupported for {}: {error}. Published files may not survive a crash",
                            directory.display()
                        );
                    });

                    return Ok(());
                }
                result => return result,
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = directory;

        Ok(())
    }
}

#[cfg(unix)]
fn unsupported_directory_sync(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Unsupported || error.raw_os_error() == Some(libc::EINVAL)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn only_unsupported_sync_errors_are_ignored() {
        for code in [libc::EINVAL, libc::EOPNOTSUPP] {
            assert!(unsupported_directory_sync(&io::Error::from_raw_os_error(
                code
            )));
        }

        for code in [libc::EIO, libc::ENOSPC, libc::EACCES, libc::EINTR] {
            assert!(!unsupported_directory_sync(&io::Error::from_raw_os_error(
                code
            )));
        }
    }
}
