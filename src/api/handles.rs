use std::path::Path;

use faccess::PathExt;
use simplelog::*;
use sqlx::{migrate::MigrateDatabase, sqlite::SqliteQueryResult, Pool, Sqlite, SqlitePool};

use crate::api::models::User;

pub fn db_path() -> Result<String, Box<dyn std::error::Error>> {
    let sys_path = Path::new("/usr/share/ffplayout");
    let mut db_path = String::from("./ffplayout.db");

    if sys_path.is_dir() && sys_path.writable() {
        db_path = String::from("/usr/share/ffplayout/ffplayout.db");
    } else if Path::new("./assets").is_dir() {
        db_path = String::from("./assets/ffplayout.db");
    }

    Ok(db_path)
}

async fn cretea_schema() -> Result<SqliteQueryResult, sqlx::Error> {
    let pool = db_connection().await?;
    let query = "PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS groups
        (
            id                      INTEGER PRIMARY KEY AUTOINCREMENT,
            name                    TEXT NOT NULL,
            UNIQUE(name)
        );
    CREATE TABLE IF NOT EXISTS settings
        (
            id                      INTEGER PRIMARY KEY AUTOINCREMENT,
            channel_name            TEXT NOT NULL,
            preview_url             TEXT NOT NULL,
            settings_path           TEXT NOT NULL,
            extra_extensions        TEXT NOT NULL,
            UNIQUE(channel_name)
        );
    CREATE TABLE IF NOT EXISTS user
        (
            id                      INTEGER PRIMARY KEY AUTOINCREMENT,
            email                   TEXT NOT NULL,
            username                TEXT NOT NULL,
            password                TEXT NOT NULL,
            salt                    TEXT NOT NULL,
            group_id                INTEGER NOT NULL DEFAULT 2,
            FOREIGN KEY (group_id)  REFERENCES groups (id) ON UPDATE SET NULL ON DELETE SET NULL,
            UNIQUE(email, username)
        );";
    let result = sqlx::query(query).execute(&pool).await;
    pool.close().await;

    result
}

pub async fn db_init() -> Result<&'static str, Box<dyn std::error::Error>> {
    let db_path = db_path()?;

    if !Sqlite::database_exists(&db_path).await.unwrap_or(false) {
        Sqlite::create_database(&db_path).await.unwrap();
        match cretea_schema().await {
            Ok(_) => info!("Database created Successfully"),
            Err(e) => panic!("{e}"),
        }
    }
    let instances = db_connection().await?;

    let query = "INSERT INTO groups(name) VALUES('admin'), ('user');
        INSERT INTO settings(channel_name, preview_url, settings_path, extra_extensions)
        VALUES('Channel 1', 'http://localhost/live/preview.m3u8',
            '/etc/ffplayout/ffplayout.yml', '.jpg,.jpeg,.png');";
    sqlx::query(query).execute(&instances).await?;

    instances.close().await;

    Ok("Database initialized!")
}

pub async fn db_connection() -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = db_path().unwrap();

    let pool = SqlitePool::connect(&db_path).await?;

    Ok(pool)
}

pub async fn add_user(
    instances: &SqlitePool,
    mail: &str,
    user: &str,
    pass: &str,
    salt: &str,
    group: &i64,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query =
        "INSERT INTO user (email, username, password, salt, group_id) VALUES($1, $2, $3, $4, $5)";
    let result = sqlx::query(query)
        .bind(mail)
        .bind(user)
        .bind(pass)
        .bind(salt)
        .bind(group)
        .execute(instances)
        .await?;

    Ok(result)
}

pub async fn get_users(
    instances: &SqlitePool,
    index: Option<i64>,
) -> Result<Vec<User>, sqlx::Error> {
    let query = match index {
        Some(i) => format!("SELECT id, email, username FROM user WHERE id = {i}"),
        None => "SELECT id, email, username FROM user".to_string(),
    };

    let result: Vec<User> = sqlx::query_as(&query).fetch_all(instances).await?;
    instances.close().await;

    Ok(result)
}

pub async fn get_login(user: &str) -> Result<Vec<User>, sqlx::Error> {
    let pool = db_connection().await?;
    let query = "SELECT id, username, password, salt FROM user WHERE username = $1";
    let result: Vec<User> = sqlx::query_as(query).bind(user).fetch_all(&pool).await?;
    pool.close().await;

    Ok(result)
}
