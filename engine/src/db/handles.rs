use actix_web::web;
use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};
use rand::{distr::Alphanumeric, Rng};
use sqlx::{sqlite::SqliteQueryResult, Pool, Row, Sqlite};

use super::models::{AdvancedConfiguration, Configuration};
use crate::db::models::{Channel, GlobalSettings, Role, TextPreset, User};
use crate::utils::{
    advanced_config::AdvancedConfig,
    config::PlayoutConfig,
    errors::{ProcessError, ServiceError},
    is_running_in_container,
};

pub async fn db_migrate(conn: &Pool<Sqlite>) -> Result<bool, ProcessError> {
    sqlx::migrate!("../migrations").run(conn).await?;
    let mut init = false;

    if select_global(conn).await.is_err() {
        let secret: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(80)
            .map(char::from)
            .collect();
        let shared = is_running_in_container().await;

        const QUERY: &str = "CREATE TRIGGER global_row_count
        BEFORE INSERT ON global
        WHEN (SELECT COUNT(*) FROM global) >= 1
        BEGIN
            SELECT RAISE(FAIL, 'Database is already initialized!');
        END;
        INSERT INTO global(secret, shared) VALUES($1, $2);";

        sqlx::query(QUERY)
            .bind(secret)
            .bind(shared)
            .execute(conn)
            .await?;

        init = true;
    }

    Ok(init)
}

pub async fn select_global(conn: &Pool<Sqlite>) -> Result<GlobalSettings, ProcessError> {
    const QUERY: &str =
        "SELECT id, secret, logs, playlists, public, storage, shared, smtp_server, smtp_user, smtp_password, smtp_starttls, smtp_port FROM global WHERE id = 1";

    let result = sqlx::query_as(QUERY).fetch_one(conn).await?;

    Ok(result)
}

pub async fn update_global(
    conn: &Pool<Sqlite>,
    global: GlobalSettings,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE global SET logs = $2, playlists = $3, public = $4, storage = $5,
            smtp_server = $6, smtp_user = $7, smtp_password = $8, smtp_starttls = $9, smtp_port = $10  WHERE id = 1";

    let result = sqlx::query(QUERY)
        .bind(global.id)
        .bind(global.logs)
        .bind(global.playlists)
        .bind(global.public)
        .bind(global.storage)
        .bind(global.smtp_server)
        .bind(global.smtp_user)
        .bind(global.smtp_password)
        .bind(global.smtp_starttls)
        .bind(global.smtp_port)
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn select_channel(conn: &Pool<Sqlite>, id: &i32) -> Result<Channel, ProcessError> {
    const QUERY: &str = "SELECT * FROM channels WHERE id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_related_channels(
    conn: &Pool<Sqlite>,
    user_id: Option<i32>,
) -> Result<Vec<Channel>, ProcessError> {
    let query = match user_id {
        Some(id) => format!(
            "SELECT c.id, c.name, c.preview_url, c.extra_extensions, c.active, c.public, c.playlists,
            c.storage, c.last_date, c.time_shift, c.timezone, c.advanced_id FROM channels c
                left join user_channels uc on uc.channel_id = c.id
                left join user u on u.id = uc.user_id
             WHERE u.id = {id} ORDER BY c.id ASC;"
        ),
        None => "SELECT * FROM channels ORDER BY id ASC;".to_string(),
    };

    let result = sqlx::query_as(&query).fetch_all(conn).await?;

    Ok(result)
}

pub async fn delete_user_channel(
    conn: &Pool<Sqlite>,
    user_id: i32,
    channel_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM user_channels WHERE user_id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(user_id)
        .bind(channel_id)
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn update_channel(
    conn: &Pool<Sqlite>,
    id: i32,
    channel: Channel,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str =
        "UPDATE channels SET name = $2, preview_url = $3, extra_extensions = $4, public = $5, playlists = $6, storage = $7, timezone = $8 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .bind(channel.timezone.map(|tz| tz.to_string()))
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn update_stat(
    conn: &Pool<Sqlite>,
    id: i32,
    last_date: &Option<String>,
    time_shift: f64,
) -> Result<SqliteQueryResult, ProcessError> {
    let query = match last_date {
        Some(_) => "UPDATE channels SET last_date = $2, time_shift = $3 WHERE id = $1",
        None => "UPDATE channels SET time_shift = $2 WHERE id = $1",
    };

    let mut q = sqlx::query(query).bind(id);

    if last_date.is_some() {
        q = q.bind(last_date);
    }

    let result = q.bind(time_shift).execute(conn).await?;

    Ok(result)
}

pub async fn update_player(
    conn: &Pool<Sqlite>,
    id: i32,
    active: bool,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE channels SET active = $2 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(active)
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn insert_channel(
    conn: &Pool<Sqlite>,
    channel: Channel,
) -> Result<Channel, ProcessError> {
    const QUERY: &str = "INSERT INTO channels (name, preview_url, extra_extensions, public, playlists, storage) VALUES($1, $2, $3, $4, $5, $6)";
    let result = sqlx::query(QUERY)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .execute(conn)
        .await?;

    let result = sqlx::query_as("SELECT * FROM channels WHERE id = $1")
        .bind(result.last_insert_rowid())
        .fetch_one(conn)
        .await?;

    Ok(result)
}

pub async fn delete_channel(
    conn: &Pool<Sqlite>,
    id: &i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM channels WHERE id = $1";

    let result = sqlx::query(QUERY).bind(id).execute(conn).await?;

    Ok(result)
}

pub async fn select_last_channel(conn: &Pool<Sqlite>) -> Result<i32, ProcessError> {
    const QUERY: &str = "select seq from sqlite_sequence WHERE name = 'channel';";

    let result = sqlx::query_scalar(QUERY).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_configuration(
    conn: &Pool<Sqlite>,
    channel: i32,
) -> Result<Configuration, ProcessError> {
    const QUERY: &str = "SELECT * FROM configurations WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_one(conn).await?;

    Ok(result)
}

pub async fn insert_configuration(
    conn: &Pool<Sqlite>,
    channel_id: i32,
    output_param: &str,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "INSERT INTO configurations (channel_id, output_param) VALUES($1, $2)";

    let result = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(output_param)
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn update_configuration(
    conn: &Pool<Sqlite>,
    id: i32,
    config: PlayoutConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE configurations SET general_stop_threshold = $2, mail_subject = $3, mail_recipient = $4, mail_level = $5, mail_interval = $6, logging_ffmpeg_level = $7, logging_ingest_level = $8, logging_detect_silence = $9, logging_ignore = $10, processing_mode = $11, processing_audio_only = $12, processing_copy_audio = $13, processing_copy_video = $14, processing_width = $15, processing_height = $16, processing_aspect = $17, processing_fps = $18, processing_add_logo = $19, processing_logo = $20, processing_logo_scale = $21, processing_logo_opacity = $22, processing_logo_position = $23, processing_audio_tracks = $24, processing_audio_track_index = $25, processing_audio_channels = $26, processing_volume = $27, processing_filter = $28, processing_override_filter = $29, processing_vtt_enable = $30, processing_vtt_dummy = $31, ingest_enable = $32, ingest_param = $33, ingest_filter = $34, playlist_day_start = $35, playlist_length = $36, playlist_infinit = $37, storage_filler = $38, storage_extensions = $39, storage_shuffle = $40, text_add = $41, text_from_filename = $42, text_font = $43, text_style = $44, text_regex = $45, task_enable = $46, task_path = $47, output_mode = $48, output_param = $49 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(config.general.stop_threshold)
        .bind(config.mail.subject)
        .bind(config.mail.recipient)
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
        .bind(config.processing.override_filter)
        .bind(config.processing.vtt_enable)
        .bind(config.processing.vtt_dummy)
        .bind(config.ingest.enable)
        .bind(config.ingest.input_param)
        .bind(config.ingest.custom_filter)
        .bind(config.playlist.day_start)
        .bind(config.playlist.length)
        .bind(config.playlist.infinit)
        .bind(config.storage.filler)
        .bind(config.storage.extensions.join(";"))
        .bind(config.storage.shuffle)
        .bind(config.text.add_text)
        .bind(config.text.text_from_filename)
        .bind(config.text.font)
        .bind(config.text.style)
        .bind(config.text.regex)
        .bind(config.task.enable)
        .bind(config.task.path.to_string_lossy().to_string())
        .bind(config.output.mode.to_string())
        .bind(config.output.output_param)
        .execute(conn)
        .await?;

    Ok(result)
}

pub async fn insert_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel_id: i32,
    adv_id: Option<i32>,
    config: AdvancedConfig,
) -> Result<i32, ProcessError> {
    const QUERY_INSERT: &str =
        "INSERT INTO advanced_configurations (channel_id, decoder_input_param, decoder_output_param, encoder_input_param,
            ingest_input_param, filter_deinterlace, filter_pad_video, filter_fps, filter_scale, filter_set_dar,
            filter_fade_in, filter_fade_out, filter_logo, filter_overlay_logo_scale, filter_overlay_logo_fade_in,
            filter_overlay_logo_fade_out, filter_overlay_logo, filter_tpad, filter_drawtext_from_file,
            filter_drawtext_from_zmq, filter_aevalsrc, filter_afade_in, filter_afade_out, filter_apad,
            filter_volume, filter_split, name)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27) RETURNING id";

    const QUERY_UPDATE: &str = "UPDATE channels SET advanced_id = $2 WHERE id = $1";

    let advanced_id: i32 = sqlx::query(QUERY_INSERT)
        .bind(channel_id)
        .bind(config.decoder.input_param)
        .bind(config.decoder.output_param)
        .bind(config.encoder.input_param)
        .bind(config.ingest.input_param)
        .bind(config.filter.deinterlace)
        .bind(config.filter.pad_video)
        .bind(config.filter.fps)
        .bind(config.filter.scale)
        .bind(config.filter.set_dar)
        .bind(config.filter.fade_in)
        .bind(config.filter.fade_out)
        .bind(config.filter.logo)
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
        .bind(config.name)
        .fetch_one(conn)
        .await?
        .get("id");

    let a_id = adv_id.unwrap_or(advanced_id);

    sqlx::query(QUERY_UPDATE)
        .bind(channel_id)
        .bind(a_id)
        .execute(conn)
        .await?;

    Ok(advanced_id)
}

pub async fn update_advanced_configuration(
    conn: &Pool<Sqlite>,
    id: i32,
    config: AdvancedConfig,
) -> Result<(), ProcessError> {
    const QUERY_ADV: &str = "UPDATE advanced_configurations SET decoder_input_param = $2, decoder_output_param = $3,
        encoder_input_param = $4, ingest_input_param = $5, filter_deinterlace = $6, filter_pad_video = $7, filter_fps = $8,
        filter_scale = $9, filter_set_dar = $10, filter_fade_in = $11, filter_fade_out = $12, filter_logo = $13,
        filter_overlay_logo_scale = $14, filter_overlay_logo_fade_in = $15, filter_overlay_logo_fade_out = $16,
        filter_overlay_logo = $17, filter_tpad = $18, filter_drawtext_from_file = $19, filter_drawtext_from_zmq = $20,
        filter_aevalsrc = $21, filter_afade_in = $22, filter_afade_out = $23, filter_apad = $24, filter_volume = $25, filter_split = $26, name = $27
        WHERE id = $1";
    const QUERY_CHL: &str = "UPDATE channels set advanced_id = $2 WHERE id = $1;";

    sqlx::query(QUERY_ADV)
        .bind(config.id)
        .bind(config.decoder.input_param)
        .bind(config.decoder.output_param)
        .bind(config.encoder.input_param)
        .bind(config.ingest.input_param)
        .bind(config.filter.deinterlace)
        .bind(config.filter.pad_video)
        .bind(config.filter.fps)
        .bind(config.filter.scale)
        .bind(config.filter.set_dar)
        .bind(config.filter.fade_in)
        .bind(config.filter.fade_out)
        .bind(config.filter.logo)
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
        .bind(config.name)
        .execute(conn)
        .await?;

    sqlx::query(QUERY_CHL)
        .bind(id)
        .bind(config.id)
        .execute(conn)
        .await?;

    Ok(())
}

pub async fn select_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel: i32,
) -> Result<AdvancedConfiguration, ProcessError> {
    const QUERY: &str = "SELECT adv.id, adv.channel_id, adv.decoder_input_param, adv.decoder_output_param, adv.encoder_input_param,
        adv.ingest_input_param, adv.filter_deinterlace, adv.filter_pad_video, adv.filter_fps,adv.filter_scale, adv.filter_set_dar,
        adv.filter_fade_in, adv.filter_fade_out, adv.filter_overlay_logo_scale, adv.filter_overlay_logo_fade_in, adv.filter_overlay_logo_fade_out,
        adv.filter_overlay_logo, adv.filter_tpad, adv.filter_drawtext_from_file, adv.filter_drawtext_from_zmq, adv.filter_aevalsrc,
        adv.filter_afade_in, adv.filter_afade_out, adv.filter_apad, adv.filter_volume, adv.filter_split, adv.filter_logo, adv.name
        FROM advanced_configurations adv left join channels ch on ch.advanced_id = adv.id WHERE ch.id = $1";

    let result = sqlx::query_as(QUERY)
        .bind(channel)
        .fetch_optional(conn)
        .await?
        .unwrap_or_default();

    Ok(result)
}

pub async fn select_related_advanced_configuration(
    conn: &Pool<Sqlite>,
    channel: i32,
) -> Result<Vec<AdvancedConfiguration>, ProcessError> {
    const QUERY: &str = "SELECT * FROM advanced_configurations WHERE channel_id = $1;";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(conn).await?;

    Ok(result)
}

pub async fn delete_advanced_configuration(
    conn: &Pool<Sqlite>,
    id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM advanced_configurations WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(conn).await?;

    Ok(result)
}

pub async fn select_role(conn: &Pool<Sqlite>, id: &i32) -> Result<Role, ProcessError> {
    const QUERY: &str = "SELECT name FROM roles WHERE id = $1";
    let result: Role = sqlx::query_as(QUERY).bind(id).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_login(conn: &Pool<Sqlite>, user: &str) -> Result<User, ProcessError> {
    const QUERY: &str =
        "SELECT u.id, u.mail, u.username, u.password, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.username = $1";

    let result = sqlx::query_as(QUERY).bind(user).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_user(conn: &Pool<Sqlite>, id: i32) -> Result<User, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(conn).await?;

    Ok(result)
}

pub async fn select_global_admins(conn: &Pool<Sqlite>) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.role_id = 1";

    let result = sqlx::query_as(QUERY).fetch_all(conn).await?;

    Ok(result)
}

pub async fn select_users(conn: &Pool<Sqlite>) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT id, username FROM user";

    let result = sqlx::query_as(QUERY).fetch_all(conn).await?;

    Ok(result)
}

pub async fn insert_user(conn: &Pool<Sqlite>, user: User) -> Result<(), ServiceError> {
    const QUERY: &str =
        "INSERT INTO user (mail, username, password, role_id) VALUES($1, $2, $3, $4) RETURNING id";

    let password_hash = web::block(move || {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(user.password.as_bytes(), &salt)
            .unwrap();

        hash.to_string()
    })
    .await?;

    let user_id: i32 = sqlx::query(QUERY)
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

pub async fn insert_or_update_user(conn: &Pool<Sqlite>, user: User) -> Result<(), ServiceError> {
    let password_hash = web::block(move || {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(user.password.as_bytes(), &salt)
            .unwrap();

        hash.to_string()
    })
    .await?;

    const QUERY: &str = "INSERT INTO user (mail, username, password, role_id) VALUES($1, $2, $3, $4)
            ON CONFLICT(username) DO UPDATE SET
                mail = excluded.mail, username = excluded.username, password = excluded.password, role_id = excluded.role_id
        RETURNING id";

    let user_id: i32 = sqlx::query(QUERY)
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
) -> Result<SqliteQueryResult, ProcessError> {
    let query = format!("UPDATE user SET {fields} WHERE id = $1");

    let result = sqlx::query(&query).bind(id).execute(conn).await?;

    Ok(result)
}

pub async fn insert_user_channel(
    conn: &Pool<Sqlite>,
    user_id: i32,
    channel_ids: Vec<i32>,
) -> Result<(), ProcessError> {
    for channel in &channel_ids {
        const QUERY: &str =
            "INSERT OR IGNORE INTO user_channels (channel_id, user_id) VALUES ($1, $2);";

        sqlx::query(QUERY)
            .bind(channel)
            .bind(user_id)
            .execute(conn)
            .await?;
    }

    Ok(())
}

pub async fn delete_user(conn: &Pool<Sqlite>, id: i32) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM user WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(conn).await?;

    Ok(result)
}

pub async fn select_presets(conn: &Pool<Sqlite>, id: i32) -> Result<Vec<TextPreset>, ProcessError> {
    const QUERY: &str = "SELECT * FROM presets WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_all(conn).await?;

    Ok(result)
}

pub async fn update_preset(
    conn: &Pool<Sqlite>,
    id: &i32,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str =
        "UPDATE presets SET name = $1, text = $2, x = $3, y = $4, fontsize = $5, line_spacing = $6,
        fontcolor = $7, alpha = $8, box = $9, boxcolor = $10, boxborderw = $11 WHERE id = $12";

    let result = sqlx::query(QUERY)
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
        .await?;

    Ok(result)
}

pub async fn insert_preset(
    conn: &Pool<Sqlite>,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str =
        "INSERT INTO presets (channel_id, name, text, x, y, fontsize, line_spacing, fontcolor, alpha, box, boxcolor, boxborderw)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)";

    let result = sqlx::query(QUERY)
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
        .await?;

    Ok(result)
}

pub async fn new_channel_presets(
    conn: &Pool<Sqlite>,
    channel_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "INSERT INTO presets (name, text, x, y, fontsize, line_spacing, fontcolor, box, boxcolor, boxborderw, alpha, channel_id)
        VALUES ('Default', 'Welcome to ffplayout messenger!', '(w-text_w)/2', '(h-text_h)/2', '24', '4', '#ffffff@0xff', '0', '#000000@0x80', '4', '1.0', $1),
        ('Empty Text', '', '0', '0', '24', '4', '#000000', '0', '#000000', '0', '0', $1),
        ('Bottom Text fade in', 'The upcoming event will be delayed by a few minutes.', '(w-text_w)/2', '(h-line_h)*0.9', '24', '4', '#ffffff', '1', '#000000@0x80', '4', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),0,if(lt(t,ld(1)+2),(t-(ld(1)+1))/1,if(lt(t,ld(1)+8),1,if(lt(t,ld(1)+9),(1-(t-(ld(1)+8)))/1,0))))', $1),
        ('Scrolling Text', 'We have a very important announcement to make.', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),w+4,w-w/12*mod(t-ld(1),12*(w+tw)/w))', '(h-line_h)*0.9', '24', '4', '#ffffff', '1', '#000000@0x80', '4', '1.0', $1);";

    let result = sqlx::query(QUERY).bind(channel_id).execute(conn).await?;

    Ok(result)
}

pub async fn delete_preset(
    conn: &Pool<Sqlite>,
    id: &i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM presets WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(conn).await?;

    Ok(result)
}
