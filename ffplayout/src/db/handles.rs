use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};

use rand::{distributions::Alphanumeric, Rng};
use simplelog::*;
use sqlx::{sqlite::SqliteQueryResult, Pool, Sqlite};
use tokio::task;

use super::models::{AdvancedConfiguration, Configuration};
use crate::db::models::{Channel, GlobalSettings, Role, TextPreset, User};
use crate::utils::local_utc_offset;

pub async fn db_migrate(conn: &Pool<Sqlite>) -> Result<&'static str, Box<dyn std::error::Error>> {
    match sqlx::migrate!("../migrations").run(conn).await {
        Ok(_) => info!("Database migration successfully"),
        Err(e) => panic!("{e}"),
    }

    if select_global(conn).await.is_err() {
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

        sqlx::query(query).bind(secret).execute(conn).await?;
    }

    Ok("Database migrated!")
}

pub async fn select_global(conn: &Pool<Sqlite>) -> Result<GlobalSettings, sqlx::Error> {
    let query = "SELECT secret, hls_path, playlist_path, storage_path, logging_path FROM global WHERE id = 1";

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
    let query =
        "UPDATE channels SET name = $2, preview_url = $3, extra_extensions = $4 WHERE id = $1";

    sqlx::query(query)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .execute(conn)
        .await
}

pub async fn update_stat(
    conn: &Pool<Sqlite>,
    id: i32,
    last_date: String,
    time_shift: f64,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE channels SET last_date = $2, time_shift = $3 WHERE id = $1";

    sqlx::query(query)
        .bind(id)
        .bind(last_date)
        .bind(time_shift)
        .execute(conn)
        .await
}

pub async fn update_player(
    conn: &Pool<Sqlite>,
    id: i32,
    active: bool,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE channels SET active = $2 WHERE id = $1";

    sqlx::query(query).bind(id).bind(active).execute(conn).await
}

pub async fn insert_channel(conn: &Pool<Sqlite>, channel: Channel) -> Result<Channel, sqlx::Error> {
    let query = "INSERT INTO channels (name, preview_url, extra_extensions) VALUES($1, $2, $3)";
    let result = sqlx::query(query)
        .bind(channel.name)
        .bind(channel.preview_url)
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
    let query = "select seq from sqlite_sequence WHERE name = 'channel';";

    sqlx::query_scalar(query).fetch_one(conn).await
}

pub async fn select_configuration(
    conn: &Pool<Sqlite>,
    channel: i32,
) -> Result<Configuration, sqlx::Error> {
    let query = "SELECT * FROM configurations WHERE channel_id = $1";

    sqlx::query_as(query).bind(channel).fetch_one(conn).await
}

pub async fn insert_configuration(
    conn: &Pool<Sqlite>,
    channel_id: i32,
    playlist_path: String,
    output_param: String,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query =
        "INSERT INTO configurations (channel_id, playlist_path, output_param) VALUES($1, $2, $3)";

    sqlx::query(query)
        .bind(channel_id)
        .bind(playlist_path)
        .bind(output_param)
        .execute(conn)
        .await
}

pub async fn update_configuration(
    conn: &Pool<Sqlite>,
    config: Configuration,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE configurations SET stop_threshold = $2, subject = $3, smtp_server = $4, sender_addr = $5, sender_pass = $6, recipient = $7, starttls = $8, mail_level = $9, interval = $10, ffmpeg_level = $11, ingest_level = $12, detect_silence = $13 , ignore_lines = $14, processing_mode = $15, audio_only = $16, copy_audio = $17, copy_video = $18, width = $19, height = $20, aspect = $21, add_logo = $22, logo = $23, logo_scale = $24, logo_opacity = $25, logo_position = $26, audio_tracks = $27, audio_track_index = $28, audio_channels = $29, volume = $30, decoder_filter = $31, ingest_enable = $32, ingest_param = $33, ingest_filter = $34, playlist_path = $35, day_start = $36, length = $37, infinit = $38, storage_path = $39, filler = $40, extensions = $41, shuffle = $42, add_text = $43, text_from_filename = $44, fontfile = $45, regex = $46, task_enable = $47, task_path = $48, output_mode = $49, output_param = $50 WHERE id = $1";

    sqlx::query(query)
        .bind(config.id)
        .bind(config.stop_threshold)
        .bind(config.subject)
        .bind(config.smtp_server)
        .bind(config.sender_addr)
        .bind(config.sender_pass)
        .bind(config.recipient)
        .bind(config.starttls)
        .bind(config.mail_level)
        .bind(config.interval)
        .bind(config.ffmpeg_level)
        .bind(config.ingest_level)
        .bind(config.detect_silence)
        .bind(config.ignore_lines)
        .bind(config.processing_mode)
        .bind(config.audio_only)
        .bind(config.copy_audio)
        .bind(config.copy_video)
        .bind(config.width)
        .bind(config.height)
        .bind(config.aspect)
        .bind(config.add_logo)
        .bind(config.logo)
        .bind(config.logo_scale)
        .bind(config.logo_opacity)
        .bind(config.logo_position)
        .bind(config.audio_tracks)
        .bind(config.audio_track_index)
        .bind(config.audio_channels)
        .bind(config.volume)
        .bind(config.decoder_filter)
        .bind(config.ingest_enable)
        .bind(config.ingest_param)
        .bind(config.ingest_filter)
        .bind(config.playlist_path)
        .bind(config.day_start)
        .bind(config.length)
        .bind(config.infinit)
        .bind(config.storage_path)
        .bind(config.filler)
        .bind(config.extensions)
        .bind(config.shuffle)
        .bind(config.add_text)
        .bind(config.text_from_filename)
        .bind(config.fontfile)
        .bind(config.regex)
        .bind(config.task_enable)
        .bind(config.task_path)
        .bind(config.output_mode)
        .bind(config.output_param)
        .execute(conn)
        .await
}

pub async fn insert_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel_id: i32,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "INSERT INTO advanced_configurations (channel_id) VALUES($1)";

    sqlx::query(query).bind(channel_id).execute(conn).await
}

pub async fn select_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel: i32,
) -> Result<AdvancedConfiguration, sqlx::Error> {
    let query = "SELECT * FROM advanced_configurations WHERE channel_id = $1";

    sqlx::query_as(query).bind(channel).fetch_one(conn).await
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
