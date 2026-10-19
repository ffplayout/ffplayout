use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use chrono::{Local, NaiveDate};
use sqlx::{SqlitePool, migrate::Migrator};
use uuid::Uuid;

use crate::utils::errors::ProcessError;

const MAX_BACKUPS: usize = 5;

/// Back up existing file databases only when this binary has pending migrations.
pub(super) async fn before_migrations(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> Result<(), ProcessError> {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(pool)
            .await?;
    if !tables.iter().any(|name| !name.starts_with("sqlite_")) {
        return Ok(());
    }
    let applied: HashSet<i64> = if tables.iter().any(|name| name == "_sqlx_migrations") {
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect()
    } else {
        HashSet::new()
    };
    if !migrator.iter().any(|migration| {
        migration.migration_type.is_up_migration() && !applied.contains(&migration.version)
    }) {
        return Ok(());
    }
    let databases: Vec<(i64, String, String)> = sqlx::query_as("PRAGMA database_list")
        .fetch_all(pool)
        .await?;
    let Some((_, _, filename)) = databases
        .into_iter()
        .find(|(_, name, filename)| name == "main" && !filename.is_empty())
    else {
        // In-memory databases have no persistent data to protect.
        return Ok(());
    };
    let database = Path::new(&filename);
    let date = Local::now().date_naive();
    let backup = backup_path(database, applied.iter().copied().max().unwrap_or(0), date)?;
    create_backup(pool, &backup).await.map_err(|error| {
        ProcessError::DB(format!(
            "Migration aborted: could not back up {} to {}: {error}",
            database.display(),
            backup.display()
        ))
    })?;
    log::info!("Database backup before migration: {}", backup.display());
    prune_backups(database, &backup)?;

    Ok(())
}

fn backup_path(database: &Path, version: i64, date: NaiveDate) -> Result<PathBuf, ProcessError> {
    let name = database
        .file_name()
        .ok_or_else(|| ProcessError::IO("Database filename unavailable".into()))?;
    let mut filename = name.to_os_string();
    filename.push(format!(".{date}.schema-{version}.bak"));

    Ok(database.with_file_name(filename))
}

struct TemporaryBackup(PathBuf);

impl Drop for TemporaryBackup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

async fn create_backup(pool: &SqlitePool, backup: &Path) -> Result<(), ProcessError> {
    // An incomplete snapshot must never replace an earlier usable backup.
    let temporary = TemporaryBackup(backup.with_extension(format!("{}.tmp", Uuid::new_v4())));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&temporary.0)?;
    // SQLite generates a consistent snapshot, including committed WAL data.
    // A plain filesystem copy of the main database would omit that data.
    sqlx::query("VACUUM main INTO ?")
        .bind(temporary.0.to_string_lossy().as_ref())
        .execute(pool)
        .await?;
    file.sync_all()?;
    fs::rename(&temporary.0, backup)?;
    #[cfg(unix)]
    if let Some(parent) = backup.parent() {
        fs::File::open(parent)?.sync_all()?;
    }

    Ok(())
}

fn prune_backups(database: &Path, newest: &Path) -> Result<(), ProcessError> {
    let parent = database.parent().unwrap_or_else(|| Path::new("."));
    let name = database
        .file_name()
        .ok_or_else(|| ProcessError::IO("Database filename unavailable".into()))?;
    let prefix = format!("{}.", name.to_string_lossy());
    let mut backups = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let path = entry.path();
        if path == newest || !entry.file_type()?.is_file() {
            continue;
        }
        let filename = entry.file_name();
        let Some(stem) = filename
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .and_then(|name| name.strip_suffix(".bak"))
        else {
            continue;
        };
        let Some((date, release)) = stem.rsplit_once(".schema-") else {
            continue;
        };
        if release.is_empty()
            || !release.bytes().all(|byte| byte.is_ascii_digit())
            || NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
        {
            continue;
        }
        backups.push((entry.metadata()?.modified()?, path));
    }
    // Keep this snapshot and the four most recently modified older backups.
    backups.sort_by(|left, right| right.cmp(left));
    for (_, path) in backups.into_iter().skip(MAX_BACKUPS - 1) {
        fs::remove_file(&path)?;
        log::info!("Removed old migration backup: {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    use super::*;
    use crate::db::handles::db_migrate;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("migration-backup-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();

            Self(path)
        }

        async fn pool(&self) -> SqlitePool {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(self.0.join("ffplayout.db"))
                        .create_if_missing(true)
                        .journal_mode(SqliteJournalMode::Wal),
                )
                .await
                .unwrap()
        }

        fn backup(&self) -> PathBuf {
            backup_path(&self.0.join("ffplayout.db"), 6, Local::now().date_naive()).unwrap()
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn pending_migration_backs_up_pre_migration_schema_and_wal_data() {
        let directory = TestDirectory::new();
        let pool = directory.pool().await;
        sqlx::migrate!("../../migrations")
            .run_to(6, &pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint = 0")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE backup_marker(value TEXT); INSERT INTO backup_marker VALUES ('committed in WAL')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            fs::metadata(directory.0.join("ffplayout.db-wal"))
                .unwrap()
                .len()
                > 0
        );
        db_migrate(&pool).await.unwrap();
        let backup = directory.backup();
        let saved = fs::read(&backup).unwrap();
        let snapshot = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&backup)
                    .read_only(true),
            )
            .await
            .unwrap();
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&snapshot)
            .await
            .unwrap();
        assert_eq!(version, 6);
        let marker: String = sqlx::query_scalar("SELECT value FROM backup_marker")
            .fetch_one(&snapshot)
            .await
            .unwrap();
        assert_eq!(marker, "committed in WAL");
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&snapshot)
            .await
            .unwrap();
        assert_eq!(integrity, "ok");
        snapshot.close().await;
        db_migrate(&pool).await.unwrap();
        assert_eq!(
            fs::read(&backup).unwrap(),
            saved,
            "no new backup without pending migrations"
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn fresh_database_does_not_create_a_backup() {
        let directory = TestDirectory::new();
        let pool = directory.pool().await;
        db_migrate(&pool).await.unwrap();
        assert!(!directory.backup().exists());
        pool.close().await;
    }

    #[tokio::test]
    async fn failed_backup_prevents_schema_changes_and_cleans_temporary_file() {
        let directory = TestDirectory::new();
        let pool = directory.pool().await;
        sqlx::migrate!("../../migrations")
            .run_to(6, &pool)
            .await
            .unwrap();
        // A directory at the final filename prevents publishing the snapshot.
        fs::create_dir(directory.backup()).unwrap();
        let error = db_migrate(&pool).await.unwrap_err();
        assert!(error.to_string().contains("Migration aborted"));
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, 6);
        assert!(!fs::read_dir(&directory.0).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "tmp")
        }));
        pool.close().await;
    }

    #[tokio::test]
    async fn same_schema_and_date_replace_backup_only_after_success() {
        let directory = TestDirectory::new();
        let pool = directory.pool().await;
        sqlx::query("CREATE TABLE marker(value INTEGER); INSERT INTO marker VALUES (1)")
            .execute(&pool)
            .await
            .unwrap();
        create_backup(&pool, &directory.backup()).await.unwrap();
        let first = fs::read(directory.backup()).unwrap();
        sqlx::query("INSERT INTO marker VALUES (2)")
            .execute(&pool)
            .await
            .unwrap();
        create_backup(&pool, &directory.backup()).await.unwrap();
        let second = fs::read(directory.backup()).unwrap();
        assert_ne!(first, second);
        pool.close().await;
        assert!(create_backup(&pool, &directory.backup()).await.is_err());
        assert_eq!(fs::read(directory.backup()).unwrap(), second);
    }

    #[test]
    fn retention_keeps_five_backups_and_leaves_unrelated_files_alone() {
        let directory = TestDirectory::new();
        let database = directory.0.join("ffplayout.db");
        let date = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let newest = backup_path(&database, 8, date).unwrap();
        assert_eq!(
            newest.file_name().unwrap(),
            "ffplayout.db.2026-10-18.schema-8.bak"
        );
        for release in 0..7 {
            fs::write(backup_path(&database, release, date).unwrap(), "backup").unwrap();
        }
        let unrelated = [
            "other.db.2026-10-18.schema-6.bak",
            "ffplayout.db.manual.bak",
            "ffplayout.db.invalid-date.schema-6.bak",
        ];
        for name in unrelated {
            fs::write(directory.0.join(name), "keep").unwrap();
        }
        fs::write(&newest, "new backup").unwrap();
        prune_backups(&database, &newest).unwrap();
        assert!(newest.exists());
        for name in unrelated {
            assert!(directory.0.join(name).exists());
        }
        assert_eq!(
            fs::read_dir(&directory.0).unwrap().count(),
            MAX_BACKUPS + unrelated.len()
        );
    }
}
