use std::{
    borrow::Cow,
    io,
    path::Path,
    sync::{LazyLock, OnceLock},
};

use faccess::PathExt;
use inquire::Confirm;
use log::*;
use sqlx::{Pool, Sqlite, SqlitePool, migrate::MigrateDatabase};

pub mod handles;
pub mod models;

use crate::{ARGS, utils::errors::ProcessError};
use models::GlobalSettings;

pub static DB_PATH: LazyLock<Result<Cow<'static, Path>, io::Error>> = LazyLock::new(|| {
    const DEFAULT_DIR: &str = "/usr/share/ffplayout/db/";
    const DEFAULT_PATH: &str = "/usr/share/ffplayout/db/ffplayout.db";
    const ASSET_DIR: &str = "./assets";
    const ASSET_PATH: &str = "./assets/ffplayout.db";
    const DB_NAME: &str = "ffplayout.db";

    let path = if let Some(path) = ARGS.db.as_deref() {
        let mut path = Cow::Borrowed(path);
        if !path.is_absolute() {
            path = std::env::current_dir()?.join(path).into();
        }
        if path.is_dir() {
            path = path.join(DB_NAME).into();
        }
        path
    } else {
        let sys_path = Path::new(DEFAULT_DIR);
        let asset_path = Path::new(ASSET_DIR);

        if !sys_path.writable() {
            error!("Path {} not writable!", sys_path.display());
        }

        if sys_path.is_dir() && sys_path.writable() {
            Path::new(DEFAULT_PATH).into()
        } else if asset_path.is_dir() {
            Path::new(ASSET_PATH).into()
        } else {
            Path::new(DB_NAME).into()
        }
    };

    if path.is_file() {
        path.access(faccess::AccessMode::WRITE)?;
    } else if let Some(p) = path.parent() {
        p.access(faccess::AccessMode::WRITE)?;
    } else {
        return Err(io::Error::other("Database path not found"));
    }

    info!("Database path: {}", path.display());

    Ok(path)
});

pub static GLOBAL_SETTINGS: OnceLock<GlobalSettings> = OnceLock::new();
pub async fn db_pool() -> Result<Pool<Sqlite>, ProcessError> {
    let db_path = DB_PATH
        .as_ref()
        .map_err(|e| ProcessError::IO(e.to_string()))?;
    let db_path = db_path.to_string_lossy();

    if !Sqlite::database_exists(&db_path).await? {
        Sqlite::create_database(&db_path).await?;
    }

    let conn = SqlitePool::connect(&db_path).await?;

    Ok(conn)
}

pub async fn db_drop() {
    let drop = Confirm::new("Drop Database: ")
        .with_default(false)
        .prompt()
        .unwrap_or(false);

    if drop {
        let db_path = DB_PATH.as_ref().unwrap();
        match Sqlite::drop_database(&db_path.to_string_lossy()).await {
            Ok(..) => println!("Successfully dropped DB"),
            Err(e) => eprintln!("{e}"),
        };
    };
}

pub async fn init_globales(conn: &Pool<Sqlite>) -> Result<(), ProcessError> {
    let config = GlobalSettings::new(conn).await;
    GLOBAL_SETTINGS
        .set(config)
        .map_err(|_| "Failed to set global settings")?;

    Ok(())
}
