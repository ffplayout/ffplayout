use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use sqlx::{
    QueryBuilder, Row, Sqlite,
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
        "SELECT u.id, u.mail, u.username, u.password, u.role_id, u.two_factor, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.username = $1
    GROUP BY u.id";

    let result = sqlx::query_as(QUERY).bind(user).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_user(pool: &SqlitePool, id: i32) -> Result<User, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, u.two_factor, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.id = $1
    GROUP BY u.id";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_global_admins(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, u.two_factor, group_concat(uc.channel_id, ',') as channel_ids FROM user u
        left join user_channels uc on uc.user_id = u.id
    WHERE u.role_id = 1
    GROUP BY u.id";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn select_users(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT id, username FROM user";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    const QUERY: &str = "INSERT INTO user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5) RETURNING id";

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
        .bind(user.two_factor)
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

    const QUERY: &str = "INSERT INTO user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5)
            ON CONFLICT(username) DO UPDATE SET
                mail = excluded.mail, username = excluded.username, password = excluded.password, role_id = excluded.role_id, two_factor = excluded.two_factor
        RETURNING id";

    let user_id: i32 = sqlx::query(QUERY)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash)
        .bind(user.role_id)
        .bind(user.two_factor)
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
    two_factor: Option<bool>,
    mail: Option<String>,
    password_hash: Option<String>,
) -> Result<(), ProcessError> {
    if two_factor.is_none() && mail.is_none() && password_hash.is_none() {
        return Ok(());
    }

    let mut query = QueryBuilder::<Sqlite>::new("UPDATE user SET ");
    let mut has_assignment = false;

    if let Some(two_factor) = two_factor {
        query.push("two_factor = ").push_bind(i32::from(two_factor));
        has_assignment = true;
    }

    if let Some(mail) = mail {
        if has_assignment {
            query.push(", ");
        }
        query.push("mail = ").push_bind(mail);
        has_assignment = true;
    }

    if let Some(password_hash) = password_hash {
        if has_assignment {
            query.push(", ");
        }
        query.push("password = ").push_bind(password_hash);
    }

    query.push(" WHERE id = ");
    query.push_bind(id);
    query.build().execute(pool).await?;

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn updates_only_two_factor() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query("CREATE TABLE user (id INTEGER PRIMARY KEY, two_factor INTEGER, mail TEXT, password TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO user (id, two_factor) VALUES (1, 1)")
            .execute(&pool)
            .await
            .unwrap();

        update_user(&pool, 1, Some(false), None, None)
            .await
            .unwrap();

        let two_factor: i32 = sqlx::query_scalar("SELECT two_factor FROM user WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(two_factor, 0);
    }

    #[tokio::test]
    async fn selects_all_global_admins_with_their_own_channels() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE user (
                id INTEGER PRIMARY KEY,
                mail TEXT,
                username TEXT,
                password TEXT,
                role_id INTEGER,
                two_factor INTEGER
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE user_channels (user_id INTEGER, channel_id INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO user (id, username, role_id, two_factor) VALUES
                (1, 'admin-one', 1, 0),
                (2, 'admin-two', 1, 0),
                (3, 'regular-user', 3, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO user_channels (user_id, channel_id) VALUES
                (1, 10),
                (2, 20),
                (3, 30)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut admins = select_global_admins(&pool).await.unwrap();
        admins.sort_by_key(|user| user.id);

        assert_eq!(admins.len(), 2);
        assert_eq!(admins[0].channel_ids, Some(vec![10]));
        assert_eq!(admins[1].channel_ids, Some(vec![20]));
    }
}
