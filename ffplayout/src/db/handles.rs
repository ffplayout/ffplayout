use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};

use rand::{distributions::Alphanumeric, Rng};
use sqlx::{sqlite::SqliteQueryResult, Pool, Row, Sqlite};
use tokio::task;

use super::models::{AdvancedConfiguration, Configuration};
use crate::db::models::{Channel, GlobalSettings, Role, TextPreset, User};
use crate::utils::{advanced_config::AdvancedConfig, config::PlayoutConfig, local_utc_offset};

pub async fn db_migrate(conn: &Pool<Sqlite>) -> Result<&'static str, Box<dyn std::error::Error>> {
    if let Err(e) = sqlx::migrate!("../migrations").run(conn).await {
        panic!("{e}");
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
    let query = "SELECT id, secret, logging_path, playlist_root, public_root, storage_root, shared_storage FROM global WHERE id = 1";

    sqlx::query_as(query).fetch_one(conn).await
}

pub async fn update_global(
    conn: &Pool<Sqlite>,
    global: GlobalSettings,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE global SET logging_path = $2, playlist_root = $3, public_root = $4, storage_root = $5, shared_storage = $6 WHERE id = 1";

    sqlx::query(query)
        .bind(global.id)
        .bind(global.logging_path)
        .bind(global.playlist_root)
        .bind(global.public_root)
        .bind(global.storage_root)
        .bind(global.shared_storage)
        .execute(conn)
        .await
}

pub async fn select_channel(conn: &Pool<Sqlite>, id: &i32) -> Result<Channel, sqlx::Error> {
    let query = "SELECT * FROM channels WHERE id = $1";
    let mut result: Channel = sqlx::query_as(query).bind(id).fetch_one(conn).await?;

    result.utc_offset = local_utc_offset();

    Ok(result)
}

pub async fn select_related_channels(
    conn: &Pool<Sqlite>,
    user_id: Option<i32>,
) -> Result<Vec<Channel>, sqlx::Error> {
    let query = match user_id {
        Some(id) => format!(
            "SELECT c.id, c.name, c.preview_url, c.extra_extensions, c.active, c.hls_path, c.playlist_path, c.storage_path, c.last_date, c.time_shift FROM channels c
                left join user_channels uc on uc.channel_id = c.id
                left join user u on u.id = uc.user_id
             WHERE u.id = {id} ORDER BY c.id ASC;"
        ),
        None => "SELECT * FROM channels ORDER BY id ASC;".to_string(),
    };

    let mut results: Vec<Channel> = sqlx::query_as(&query).fetch_all(conn).await?;

    for result in results.iter_mut() {
        result.utc_offset = local_utc_offset();
    }

    Ok(results)
}

pub async fn delete_user_channel(
    conn: &Pool<Sqlite>,
    user_id: i32,
    channel_id: i32,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "DELETE FROM user_channels WHERE user_id = $1 AND channel_id = $2";

    sqlx::query(query)
        .bind(user_id)
        .bind(channel_id)
        .execute(conn)
        .await
}

pub async fn update_channel(
    conn: &Pool<Sqlite>,
    id: i32,
    channel: Channel,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query =
        "UPDATE channels SET name = $2, preview_url = $3, extra_extensions = $4, hls_path = $5, playlist_path = $6, storage_path = $7 WHERE id = $1";

    sqlx::query(query)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.hls_path)
        .bind(channel.playlist_path)
        .bind(channel.storage_path)
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
    let query = "INSERT INTO channels (name, preview_url, extra_extensions, hls_path, playlist_path, storage_path) VALUES($1, $2, $3, $4, $5, $6)";
    let result = sqlx::query(query)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.hls_path)
        .bind(channel.playlist_path)
        .bind(channel.storage_path)
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
    output_param: String,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "INSERT INTO configurations (channel_id, output_param) VALUES($1, $2)";

    sqlx::query(query)
        .bind(channel_id)
        .bind(output_param)
        .execute(conn)
        .await
}

pub async fn update_configuration(
    conn: &Pool<Sqlite>,
    id: i32,
    config: PlayoutConfig,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE configurations SET general_stop_threshold = $2, mail_subject = $3, mail_smtp = $4, mail_addr = $5, mail_pass = $6, mail_recipient = $7, mail_starttls = $8, mail_level = $9, mail_interval = $10, logging_ffmpeg_level = $11, logging_ingest_level = $12, logging_detect_silence = $13, logging_ignore = $14, processing_mode = $15, processing_audio_only = $16, processing_copy_audio = $17, processing_copy_video = $18, processing_width = $19, processing_height = $20, processing_aspect = $21, processing_fps = $22, processing_add_logo = $23, processing_logo = $24, processing_logo_scale = $25, processing_logo_opacity = $26, processing_logo_position = $27, processing_audio_tracks = $28, processing_audio_track_index = $29, processing_audio_channels = $30, processing_volume = $31, processing_filter = $32, ingest_enable = $33, ingest_param = $34, ingest_filter = $35, playlist_day_start = $36, playlist_length = $37, playlist_infinit = $38, storage_filler = $39, storage_extensions = $40, storage_shuffle = $41, text_add = $42, text_from_filename = $43, text_font = $44, text_style = $45, text_regex = $46, task_enable = $47, task_path = $48, output_mode = $49, output_param = $50 WHERE id = $1";

    sqlx::query(query)
        .bind(id)
        .bind(config.general.stop_threshold)
        .bind(config.mail.subject)
        .bind(config.mail.smtp_server)
        .bind(config.mail.sender_addr)
        .bind(config.mail.sender_pass)
        .bind(config.mail.recipient)
        .bind(config.mail.starttls)
        .bind(config.mail.mail_level.as_str())
        .bind(config.mail.interval)
        .bind(config.logging.ffmpeg_level)
        .bind(config.logging.ingest_level)
        .bind(config.logging.detect_silence)
        .bind(config.logging.ignore_lines.join(";"))
        .bind(config.processing.mode.to_string())
        .bind(config.processing.audio_only)
        .bind(config.processing.copy_audio)
        .bind(config.processing.copy_video)
        .bind(config.processing.width)
        .bind(config.processing.height)
        .bind(config.processing.aspect)
        .bind(config.processing.fps)
        .bind(config.processing.add_logo)
        .bind(config.processing.logo)
        .bind(config.processing.logo_scale)
        .bind(config.processing.logo_opacity)
        .bind(config.processing.logo_position)
        .bind(config.processing.audio_tracks)
        .bind(config.processing.audio_track_index)
        .bind(config.processing.audio_channels)
        .bind(config.processing.volume)
        .bind(config.processing.custom_filter)
        .bind(config.ingest.enable)
        .bind(config.ingest.input_param)
        .bind(config.ingest.custom_filter)
        .bind(config.playlist.day_start)
        .bind(config.playlist.length)
        .bind(config.playlist.infinit)
        .bind(config.storage.filler.to_string_lossy().to_string())
        .bind(config.storage.extensions.join(";"))
        .bind(config.storage.shuffle)
        .bind(config.text.add_text)
        .bind(config.text.text_from_filename)
        .bind(config.text.fontfile)
        .bind(config.text.style)
        .bind(config.text.regex)
        .bind(config.task.enable)
        .bind(config.task.path.to_string_lossy().to_string())
        .bind(config.output.mode.to_string())
        .bind(config.output.output_param)
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

pub async fn update_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel_id: i32,
    config: AdvancedConfig,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "UPDATE advanced_configurations SET decoder_input_param = $2, decoder_output_param = $3, encoder_input_param = $4, ingest_input_param = $5, filter_deinterlace = $6, filter_pad_scale_w = $7, filter_pad_scale_h = $8, filter_pad_video = $9, filter_fps = $10, filter_scale = $11, filter_set_dar = $12, filter_fade_in = $13, filter_fade_out = $14, filter_overlay_logo_scale = $15, filter_overlay_logo_fade_in = $16, filter_overlay_logo_fade_out = $17, filter_overlay_logo = $18, filter_tpad = $19, filter_drawtext_from_file = $20, filter_drawtext_from_zmq = $21, filter_aevalsrc = $22, filter_afade_in = $23, filter_afade_out = $24, filter_apad = $25, filter_volume = $26, filter_split = $27 WHERE channel_id = $1";

    sqlx::query(query)
        .bind(channel_id)
        .bind(config.decoder.input_param)
        .bind(config.decoder.output_param)
        .bind(config.encoder.input_param)
        .bind(config.ingest.input_param)
        .bind(config.filter.deinterlace)
        .bind(config.filter.pad_scale_w)
        .bind(config.filter.pad_scale_h)
        .bind(config.filter.pad_video)
        .bind(config.filter.fps)
        .bind(config.filter.scale)
        .bind(config.filter.set_dar)
        .bind(config.filter.fade_in)
        .bind(config.filter.fade_out)
        .bind(config.filter.overlay_logo_scale)
        .bind(config.filter.overlay_logo_fade_in)
        .bind(config.filter.overlay_logo_fade_out)
        .bind(config.filter.overlay_logo)
        .bind(config.filter.tpad)
        .bind(config.filter.drawtext_from_file)
        .bind(config.filter.drawtext_from_zmq)
        .bind(config.filter.aevalsrc)
        .bind(config.filter.afade_in)
        .bind(config.filter.afade_out)
        .bind(config.filter.apad)
        .bind(config.filter.volume)
        .bind(config.filter.split)
        .execute(conn)
        .await
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
    let query =
        "SELECT u.id, u.mail, u.username, u.password, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.username = $1";

    sqlx::query_as(query).bind(user).fetch_one(conn).await
}

pub async fn select_user(conn: &Pool<Sqlite>, id: i32) -> Result<User, sqlx::Error> {
    let query = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.id = $1";

    sqlx::query_as(query).bind(id).fetch_one(conn).await
}

pub async fn select_global_admins(conn: &Pool<Sqlite>) -> Result<Vec<User>, sqlx::Error> {
    let query = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.role_id = 1";

    sqlx::query_as(query).fetch_all(conn).await
}

pub async fn select_users(conn: &Pool<Sqlite>) -> Result<Vec<User>, sqlx::Error> {
    let query = "SELECT id, username FROM user";

    sqlx::query_as(query).fetch_all(conn).await
}

pub async fn insert_user(conn: &Pool<Sqlite>, user: User) -> Result<(), sqlx::Error> {
    let password_hash = task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(user.password.clone().as_bytes(), &salt)
            .unwrap();

        hash.to_string()
    })
    .await
    .unwrap();

    let query =
        "INSERT INTO user (mail, username, password, role_id) VALUES($1, $2, $3, $4) RETURNING id";

    let user_id: i32 = sqlx::query(query)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash)
        .bind(user.role_id)
        .fetch_one(conn)
        .await?
        .get("id");

    if let Some(channel_ids) = user.channel_ids {
        insert_user_channel(conn, user_id, channel_ids).await?;
    }

    Ok(())
}

pub async fn update_user(
    conn: &Pool<Sqlite>,
    id: i32,
    fields: String,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = format!("UPDATE user SET {fields} WHERE id = $1");

    sqlx::query(&query).bind(id).execute(conn).await
}

pub async fn insert_user_channel(
    conn: &Pool<Sqlite>,
    user_id: i32,
    channel_ids: Vec<i32>,
) -> Result<(), sqlx::Error> {
    for channel in &channel_ids {
        let query = "INSERT OR IGNORE INTO user_channels (channel_id, user_id) VALUES ($1, $2);";

        sqlx::query(query)
            .bind(channel)
            .bind(user_id)
            .execute(conn)
            .await?;
    }

    Ok(())
}

pub async fn delete_user(conn: &Pool<Sqlite>, id: i32) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "DELETE FROM user WHERE id = $1;";

    sqlx::query(query).bind(id).execute(conn).await
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
