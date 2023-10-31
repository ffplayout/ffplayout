use sqlx::{Pool, Sqlite, SqlitePool};

pub mod handles;
pub mod models;

use crate::utils::db_path;

pub async fn db_pool(db: &Option<String>) -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = db_path(db.clone()).unwrap();
    let conn = SqlitePool::connect(db_path).await?;

    Ok(conn)
}
