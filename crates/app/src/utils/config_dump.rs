//! JSON settings files. Database access stays in db::handles.
use std::path::{Path, PathBuf};

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{
    db::{handles, models::ConfigDump},
    utils::errors::ProcessError,
};

const MAX_DUMP_BYTES: u64 = 32 * 1024 * 1024;

pub async fn dump(pool: &sqlx::SqlitePool) -> Result<PathBuf, ProcessError> {
    let dump = handles::config_dump::export(pool).await?;
    let path = PathBuf::from("ffplayout.json");
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let contents = serde_json::to_vec_pretty(&dump)?;

    if contents.len() as u64 + 1 > MAX_DUMP_BYTES {
        return Err(ProcessError::Custom("Settings dump exceeds 32 MiB".into()));
    }

    let mut options = fs::OpenOptions::new();

    options.write(true).create_new(true);

    #[cfg(unix)]
    options.mode(0o600);

    let result = async {
        let mut file = options.open(&temp).await?;

        file.write_all(&contents).await?;
        file.write_all(b"\n").await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&temp, &path).await?;

        Ok::<_, std::io::Error>(())
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(&temp).await;
    }

    result?;

    Ok(path)
}

pub async fn import(pool: &sqlx::SqlitePool, path: &Path) -> Result<(), ProcessError> {
    let mut file = fs::File::open(path).await?.take(MAX_DUMP_BYTES + 1);
    let mut contents = Vec::new();

    file.read_to_end(&mut contents).await?;

    if contents.len() as u64 > MAX_DUMP_BYTES {
        return Err(ProcessError::Custom("Settings dump exceeds 32 MiB".into()));
    }

    let dump: ConfigDump = serde_json::from_slice(&contents)
        .map_err(|error| ProcessError::Custom(format!("Invalid JSON settings dump: {error}")))?;

    handles::config_dump::restore(pool, dump).await
}
