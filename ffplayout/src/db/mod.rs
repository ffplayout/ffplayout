use sqlx::{migrate::MigrateDatabase, Pool, Sqlite, SqlitePool};

pub mod handles;
pub mod models;

use crate::utils::db_path;

pub async fn db_pool() -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = db_path().unwrap();

    if !Sqlite::database_exists(db_path).await.unwrap_or(false) {
        Sqlite::create_database(db_path).await.unwrap();
    }

    let conn = SqlitePool::connect(db_path).await?;

    Ok(conn)
}
