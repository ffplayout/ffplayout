use std::path::Path;

use faccess::PathExt;
use rand::{distributions::Alphanumeric, Rng};
use simplelog::*;
use sqlx::{migrate::MigrateDatabase, sqlite::SqliteQueryResult, Pool, Sqlite, SqlitePool};

use crate::api::models::User;
use crate::api::utils::GlobalSettings;

#[derive(Debug, sqlx::FromRow)]
struct Role {
    name: String,
}

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
    let conn = db_connection().await?;
    let query = "PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS global
        (
            id                      INTEGER PRIMARY KEY AUTOINCREMENT,
            secret                  TEXT NOT NULL,
            UNIQUE(secret)
        );
    CREATE TABLE IF NOT EXISTS roles
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
            role_id                 INTEGER NOT NULL DEFAULT 2,
            FOREIGN KEY (role_id)   REFERENCES roles (id) ON UPDATE SET NULL ON DELETE SET NULL,
            UNIQUE(email, username)
        );";
    let result = sqlx::query(query).execute(&conn).await;
    conn.close().await;

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
    let secret: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(80)
        .map(char::from)
        .collect();

    let instances = db_connection().await?;

    let query = "CREATE TRIGGER global_row_count
        BEFORE INSERT ON global
        WHEN (SELECT COUNT(*) FROM global) >= 1
        BEGIN
            SELECT RAISE(FAIL, 'Database is already init!');
        END;
        INSERT INTO global(secret) VALUES($1);
        INSERT INTO roles(name) VALUES('admin'), ('user'), ('guest');
        INSERT INTO settings(channel_name, preview_url, settings_path, extra_extensions)
        VALUES('Channel 1', 'http://localhost/live/preview.m3u8',
            '/etc/ffplayout/ffplayout.yml', '.jpg,.jpeg,.png');";
    sqlx::query(query).bind(secret).execute(&instances).await?;
    instances.close().await;

    Ok("Database initialized!")
}

pub async fn db_connection() -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = db_path().unwrap();
    let conn = SqlitePool::connect(&db_path).await?;

    Ok(conn)
}

pub async fn get_global() -> Result<GlobalSettings, sqlx::Error> {
    let conn = db_connection().await?;
    let query = "SELECT secret FROM global WHERE id = 1";
    let result: GlobalSettings = sqlx::query_as(query).fetch_one(&conn).await?;
    conn.close().await;

    Ok(result)
}

pub async fn get_role(id: &i64) -> Result<String, sqlx::Error> {
    let conn = db_connection().await?;
    let query = "SELECT name FROM roles WHERE id = $1";
    let result: Role = sqlx::query_as(query).bind(id).fetch_one(&conn).await?;
    conn.close().await;

    Ok(result.name)
}

pub async fn add_user(
    mail: &str,
    user: &str,
    pass: &str,
    salt: &str,
    group: &i64,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let conn = db_connection().await?;
    let query =
        "INSERT INTO user (email, username, password, salt, role_id) VALUES($1, $2, $3, $4, $5)";
    let result = sqlx::query(query)
        .bind(mail)
        .bind(user)
        .bind(pass)
        .bind(salt)
        .bind(group)
        .execute(&conn)
        .await?;
    conn.close().await;

    Ok(result)
}

pub async fn get_login(user: &str) -> Result<User, sqlx::Error> {
    let conn = db_connection().await?;
    let query = "SELECT id, email, username, password, salt, role_id FROM user WHERE username = $1";
    let result: User = sqlx::query_as(query).bind(user).fetch_one(&conn).await?;
    conn.close().await;

    Ok(result)
}
