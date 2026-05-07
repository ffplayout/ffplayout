use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use sqlx::{
    Row,
    sqlite::{SqlitePool, SqliteQueryResult},
};
use tokio::task;

use crate::{
    db::models::{Role, User},
    utils::errors::{ProcessError, ServiceError},
};

pub async fn select_role(pool: &SqlitePool, id: &i32) -> Result<Role, ProcessError> {
    const QUERY: &str = "SELECT name FROM roles WHERE id = $1";
    let result: Role = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_login(pool: &SqlitePool, user: &str) -> Result<User, ProcessError> {
    const QUERY: &str =
        "SELECT u.id, u.mail, u.username, u.password, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.username = $1";

    let result = sqlx::query_as(QUERY).bind(user).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_user(pool: &SqlitePool, id: i32) -> Result<User, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_global_admins(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.role_id = 1";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn select_users(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT id, username FROM user";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    const QUERY: &str =
        "INSERT INTO user (mail, username, password, role_id) VALUES($1, $2, $3, $4) RETURNING id";

    let password_hash = task::spawn_blocking(move || {
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
        .fetch_one(pool)
        .await?
        .get("id");

    if let Some(channel_ids) = user.channel_ids {
        insert_user_channel(pool, user_id, channel_ids).await?;
    }

    Ok(())
}

pub async fn insert_or_update_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    let password_hash = task::spawn_blocking(move || {
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
        .fetch_one(pool)
        .await?
        .get("id");

    if let Some(channel_ids) = user.channel_ids {
        insert_user_channel(pool, user_id, channel_ids).await?;
    }

    Ok(())
}

pub async fn update_user(
    pool: &SqlitePool,
    id: i32,
    fields: String,
) -> Result<SqliteQueryResult, ProcessError> {
    let query = format!("UPDATE user SET {fields} WHERE id = $1");

    let result = sqlx::query(&query).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn delete_user(pool: &SqlitePool, id: i32) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM user WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn insert_user_channel(
    pool: &SqlitePool,
    user_id: i32,
    channel_ids: Vec<i32>,
) -> Result<(), ProcessError> {
    for channel in &channel_ids {
        const QUERY: &str =
            "INSERT OR IGNORE INTO user_channels (channel_id, user_id) VALUES ($1, $2);";

        sqlx::query(QUERY)
            .bind(channel)
            .bind(user_id)
            .execute(pool)
            .await?;
    }

    Ok(())
}
