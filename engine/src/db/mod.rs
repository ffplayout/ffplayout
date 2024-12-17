use std::{
    io::{stdin, stdout, Write},
    sync::OnceLock,
};

use sqlx::{migrate::MigrateDatabase, Pool, Sqlite, SqlitePool};

pub mod handles;
pub mod models;

use crate::utils::DB_PATH;
use models::GlobalSettings;

pub static GLOBAL_SETTINGS: OnceLock<GlobalSettings> = OnceLock::new();
pub async fn db_pool() -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = DB_PATH.as_ref().unwrap();
    let db_path = db_path.to_string_lossy();

    if !Sqlite::database_exists(&db_path).await.unwrap_or(false) {
        Sqlite::create_database(&db_path).await.unwrap();
    }

    let conn = SqlitePool::connect(&db_path).await?;

    Ok(conn)
}

pub async fn db_drop() {
    let mut drop_answer = String::new();

    print!("Drop Database [Y/n]: ");
    stdout().flush().unwrap();

    stdin()
        .read_line(&mut drop_answer)
        .expect("Did not enter a yes or no?");

    let drop = drop_answer.trim().to_lowercase().starts_with('y');

    if drop {
        let db_path = DB_PATH.as_ref().unwrap();
        match Sqlite::drop_database(&db_path.to_string_lossy()).await {
            Ok(_) => println!("Successfully dropped DB"),
            Err(e) => eprintln!("{e}"),
        };
    };
}

pub async fn init_globales(conn: &Pool<Sqlite>) -> Result<(), Box<dyn std::error::Error>> {
    let config = GlobalSettings::new(conn).await;
    GLOBAL_SETTINGS
        .set(config)
        .map_err(|_| "Failed to set global settings")?;

    Ok(())
}
