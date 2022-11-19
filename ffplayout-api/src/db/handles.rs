use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};

use rand::{distributions::Alphanumeric, Rng};
use simplelog::*;
use sqlx::{migrate::MigrateDatabase, sqlite::SqliteQueryResult, Pool, Sqlite};

use crate::db::models::{Channel, TextPreset, User};
use crate::utils::{db_path, local_utc_offset, GlobalSettings};

#[derive(Debug, sqlx::FromRow)]
struct Role {
    name: String,
}

async fn create_schema(conn: &Pool<Sqlite>) -> Result<SqliteQueryResult, sqlx::Error> {
    let query = "PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS global
        (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            secret                   TEXT NOT NULL,
            UNIQUE(secret)
        );
    CREATE TABLE IF NOT EXISTS roles
        (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            name                     TEXT NOT NULL,
            UNIQUE(name)
        );
    CREATE TABLE IF NOT EXISTS channels
        (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            name                     TEXT NOT NULL,
            preview_url              TEXT NOT NULL,
            config_path              TEXT NOT NULL,
            extra_extensions         TEXT NOT NULL,
            service                  TEXT NOT NULL,
            UNIQUE(name, service)
        );
    CREATE TABLE IF NOT EXISTS presets
        (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            name                     TEXT NOT NULL,
            text                     TEXT NOT NULL,
            x                        TEXT NOT NULL,
            y                        TEXT NOT NULL,
            fontsize                 TEXT NOT NULL,
            line_spacing             TEXT NOT NULL,
            fontcolor                TEXT NOT NULL,
            box                      TEXT NOT NULL,
            boxcolor                 TEXT NOT NULL,
            boxborderw               TEXT NOT NULL,
            alpha                    TEXT NOT NULL,
            channel_id               INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE SET NULL ON DELETE SET NULL,
            UNIQUE(name)
        );
    CREATE TABLE IF NOT EXISTS user
        (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            mail                     TEXT NOT NULL,
            username                 TEXT NOT NULL,
            password                 TEXT NOT NULL,
            salt                     TEXT NOT NULL,
            role_id                  INTEGER NOT NULL DEFAULT 2,
            channel_id               INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (role_id)    REFERENCES roles (id) ON UPDATE SET NULL ON DELETE SET NULL,
            FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE SET NULL ON DELETE SET NULL,
            UNIQUE(mail, username)
        );";

    sqlx::query(query).execute(conn).await
}

pub async fn db_init(
    conn: &Pool<Sqlite>,
    domain: Option<String>,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let db_path = db_path()?;

    if !Sqlite::database_exists(&db_path).await.unwrap_or(false) {
        Sqlite::create_database(&db_path).await.unwrap();
        match create_schema(conn).await {
            Ok(_) => info!("Database created Successfully"),
            Err(e) => panic!("{e}"),
        }
    }
    let secret: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(80)
        .map(char::from)
        .collect();

    let url = match domain {
        Some(d) => format!("http://{d}/live/stream.m3u8"),
        None => "http://localhost/live/stream.m3u8".to_string(),
    };

    let query = "CREATE TRIGGER global_row_count
        BEFORE INSERT ON global
        WHEN (SELECT COUNT(*) FROM global) >= 1
        BEGIN
            SELECT RAISE(FAIL, 'Database is already initialized!');
        END;
        INSERT INTO global(secret) VALUES($1);
        INSERT INTO channels(name, preview_url, config_path, extra_extensions, service)
        VALUES('Channel 1', $2, '/etc/ffplayout/ffplayout.yml', 'jpg,jpeg,png', 'ffplayout.service');
        INSERT INTO roles(name) VALUES('admin'), ('user'), ('guest');
        INSERT INTO presets(name, text, x, y, fontsize, line_spacing, fontcolor, box, boxcolor, boxborderw, alpha, channel_id)
        VALUES('Default', 'Wellcome to ffplayout messenger!', '(w-text_w)/2', '(h-text_h)/2', '24', '4', '#ffffff@0xff', '0', '#000000@0x80', '4', '1.0', '1'),
        ('Empty Text', '', '0', '0', '24', '4', '#000000', '0', '#000000', '0', '0', '1'),
        ('Bottom Text fade in', 'The upcoming event will be delayed by a few minutes.', '(w-text_w)/2', '(h-line_h)*0.9', '24', '4', '#ffffff',
            '1', '#000000@0x80', '4', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),0,if(lt(t,ld(1)+2),(t-(ld(1)+1))/1,if(lt(t,ld(1)+8),1,if(lt(t,ld(1)+9),(1-(t-(ld(1)+8)))/1,0))))', '1'),
        ('Scrolling Text', 'We have a very important announcement to make.', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),w+4,w-w/12*mod(t-ld(1),12*(w+tw)/w))', '(h-line_h)*0.9',
            '24', '4', '#ffffff', '1', '#000000@0x80', '4', '1.0', '1');";
    sqlx::query(query)
        .bind(secret)
        .bind(url)
        .execute(conn)
        .await?;

    Ok("Database initialized!")
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
    let query = "INSERT INTO channels (name, preview_url, config_path, extra_extensions, service) VALUES($1, $2, $3, $4, $5)";
    let result = sqlx::query(query)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.config_path)
        .bind(channel.extra_extensions)
        .bind(channel.service)
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

pub async fn select_role(conn: &Pool<Sqlite>, id: &i32) -> Result<String, sqlx::Error> {
    let query = "SELECT name FROM roles WHERE id = $1";
    let result: Role = sqlx::query_as(query).bind(id).fetch_one(conn).await?;

    Ok(result.name)
}

pub async fn select_login(conn: &Pool<Sqlite>, user: &str) -> Result<User, sqlx::Error> {
    let query = "SELECT id, mail, username, password, salt, role_id FROM user WHERE username = $1";

    sqlx::query_as(query).bind(user).fetch_one(conn).await
}

pub async fn select_user(conn: &Pool<Sqlite>, user: &str) -> Result<User, sqlx::Error> {
    let query = "SELECT id, mail, username, role_id FROM user WHERE username = $1";

    sqlx::query_as(query).bind(user).fetch_one(conn).await
}

pub async fn insert_user(
    conn: &Pool<Sqlite>,
    user: User,
) -> Result<SqliteQueryResult, sqlx::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(user.password.clone().as_bytes(), &salt)
        .unwrap();

    let query =
        "INSERT INTO user (mail, username, password, salt, role_id) VALUES($1, $2, $3, $4, $5)";

    sqlx::query(query)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash.to_string())
        .bind(salt.to_string())
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
        fontcolor = $7, alpha = $8, box = $9, boxcolor = $10, boxborderw = 11 WHERE id = $12";

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
