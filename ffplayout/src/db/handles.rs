use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};

use rand::{distributions::Alphanumeric, Rng};
use simplelog::*;
use sqlx::{migrate::MigrateDatabase, sqlite::SqliteQueryResult, Pool, Sqlite};
use tokio::task;

use crate::db::{
    db_pool,
    models::{Channel, TextPreset, User},
};
use crate::utils::{db_path, local_utc_offset, GlobalSettings, Role};

pub async fn db_migrate() -> Result<&'static str, Box<dyn std::error::Error>> {
    let db_path = db_path()?;

    if !Sqlite::database_exists(db_path).await.unwrap_or(false) {
        Sqlite::create_database(db_path).await.unwrap();
    }

    let pool = db_pool().await?;

    match sqlx::migrate!("../migrations").run(&pool).await {
        Ok(_) => info!("Database migration successfully"),
        Err(e) => panic!("{e}"),
    }

    if let Err(_) = select_global(&pool).await {
        let secret: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(80)
            .map(char::from)
            .collect();

        let query = "CREATE TRIGGER global_row_count
        BEFORE INSERT ON global
        WHEN (SELECT COUNT(*) FROM global) >= 1
        BEGIN
            SELECT RAISE(FAIL, 'Database is already initialized!');
        END;
        INSERT INTO global(secret) VALUES($1);";

        sqlx::query(query).bind(secret).execute(&pool).await?;
    }

    Ok("Database migrated!")
}

pub async fn select_global(conn: &Pool<Sqlite>) -> Result<GlobalSettings, sqlx::Error> {
    let query = "SELECT secret FROM global WHERE id = 1";

    sqlx::query_as(query).fetch_one(conn).await
}

pub async fn select_channel(conn: &Pool<Sqlite>, id: &i32) -> Result<Channel, sqlx::Error> {
    let query = "SELECT * FROM channels WHERE id = $1";
    let mut result: Channel = sqlx::query_as(query).bind(id).fetch_one(conn).await?;

    result.utc_offset = local_utc_offset();

    Ok(result)
}

pub async fn select_all_channels(conn: &Pool<Sqlite>) -> Result<Vec<Channel>, sqlx::Error> {
    let query = "SELECT * FROM channels";
    let mut results: Vec<Channel> = sqlx::query_as(query).fetch_all(conn).await?;

    for result in results.iter_mut() {
        result.utc_offset = local_utc_offset();
    }

    Ok(results)
}

pub async fn update_channel(
    conn: &Pool<Sqlite>,
    id: i32,
    channel: Channel,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE channels SET name = $2, preview_url = $3, config_path = $4, extra_extensions = $5 WHERE id = $1";

    sqlx::query(query)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.config_path)
        .bind(channel.extra_extensions)
        .execute(conn)
        .await
}

pub async fn insert_channel(conn: &Pool<Sqlite>, channel: Channel) -> Result<Channel, sqlx::Error> {
    let query = "INSERT INTO channels (name, preview_url, config_path, extra_extensions) VALUES($1, $2, $3, $4)";
    let result = sqlx::query(query)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.config_path)
        .bind(channel.extra_extensions)
        .execute(conn)
        .await?;

    sqlx::query_as("SELECT * FROM channels WHERE id = $1")
        .bind(result.last_insert_rowid())
        .fetch_one(conn)
        .await
}

pub async fn delete_channel(
    conn: &Pool<Sqlite>,
    id: &i32,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "DELETE FROM channels WHERE id = $1";

    sqlx::query(query).bind(id).execute(conn).await
}

pub async fn select_last_channel(conn: &Pool<Sqlite>) -> Result<i32, sqlx::Error> {
    let query = "SELECT id FROM channels ORDER BY id DESC LIMIT 1;";

    sqlx::query_scalar(query).fetch_one(conn).await
}

pub async fn select_role(conn: &Pool<Sqlite>, id: &i32) -> Result<Role, sqlx::Error> {
    let query = "SELECT name FROM roles WHERE id = $1";
    let result: Role = sqlx::query_as(query).bind(id).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_login(conn: &Pool<Sqlite>, user: &str) -> Result<User, sqlx::Error> {
    let query = "SELECT id, mail, username, password, role_id FROM user WHERE username = $1";

    sqlx::query_as(query).bind(user).fetch_one(conn).await
}

pub async fn select_user(conn: &Pool<Sqlite>, user: &str) -> Result<User, sqlx::Error> {
    let query = "SELECT id, mail, username, role_id FROM user WHERE username = $1";

    sqlx::query_as(query).bind(user).fetch_one(conn).await
}

pub async fn select_user_by_id(conn: &Pool<Sqlite>, id: i32) -> Result<User, sqlx::Error> {
    let query = "SELECT id, mail, username, role_id FROM user WHERE id = $1";

    sqlx::query_as(query).bind(id).fetch_one(conn).await
}

pub async fn select_users(conn: &Pool<Sqlite>) -> Result<Vec<User>, sqlx::Error> {
    let query = "SELECT id, username FROM user";

    sqlx::query_as(query).fetch_all(conn).await
}

pub async fn insert_user(
    conn: &Pool<Sqlite>,
    user: User,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let password_hash = task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(user.password.clone().as_bytes(), &salt)
            .unwrap();

        hash.to_string()
    })
    .await
    .unwrap();

    let query = "INSERT INTO user (mail, username, password, role_id) VALUES($1, $2, $3, $4)";

    sqlx::query(query)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash)
        .bind(user.role_id)
        .execute(conn)
        .await
}

pub async fn update_user(
    conn: &Pool<Sqlite>,
    id: i32,
    fields: String,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = format!("UPDATE user SET {fields} WHERE id = $1");

    sqlx::query(&query).bind(id).execute(conn).await
}

pub async fn delete_user(
    conn: &Pool<Sqlite>,
    name: &str,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "DELETE FROM user WHERE username = $1;";

    sqlx::query(query).bind(name).execute(conn).await
}

pub async fn select_presets(conn: &Pool<Sqlite>, id: i32) -> Result<Vec<TextPreset>, sqlx::Error> {
    let query = "SELECT * FROM presets WHERE channel_id = $1";

    sqlx::query_as(query).bind(id).fetch_all(conn).await
}

pub async fn update_preset(
    conn: &Pool<Sqlite>,
    id: &i32,
    preset: TextPreset,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query =
        "UPDATE presets SET name = $1, text = $2, x = $3, y = $4, fontsize = $5, line_spacing = $6,
        fontcolor = $7, alpha = $8, box = $9, boxcolor = $10, boxborderw = $11 WHERE id = $12";

    sqlx::query(query)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.x)
        .bind(preset.y)
        .bind(preset.fontsize)
        .bind(preset.line_spacing)
        .bind(preset.fontcolor)
        .bind(preset.alpha)
        .bind(preset.r#box)
        .bind(preset.boxcolor)
        .bind(preset.boxborderw)
        .bind(id)
        .execute(conn)
        .await
}

pub async fn insert_preset(
    conn: &Pool<Sqlite>,
    preset: TextPreset,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query =
        "INSERT INTO presets (channel_id, name, text, x, y, fontsize, line_spacing, fontcolor, alpha, box, boxcolor, boxborderw)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)";

    sqlx::query(query)
        .bind(preset.channel_id)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.x)
        .bind(preset.y)
        .bind(preset.fontsize)
        .bind(preset.line_spacing)
        .bind(preset.fontcolor)
        .bind(preset.alpha)
        .bind(preset.r#box)
        .bind(preset.boxcolor)
        .bind(preset.boxborderw)
        .execute(conn)
        .await
}

pub async fn delete_preset(
    conn: &Pool<Sqlite>,
    id: &i32,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "DELETE FROM presets WHERE id = $1;";

    sqlx::query(query).bind(id).execute(conn).await
}
