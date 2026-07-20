use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use sqlx::{
    Executor, QueryBuilder, Row, Sqlite,
    sqlite::{SqliteConnection, SqlitePool, SqliteQueryResult},
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

pub async fn select_users(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT id, username FROM user";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    const QUERY: &str = "INSERT INTO user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5) RETURNING id";

    let password_hash = task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(user.password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await?
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    let mut transaction = pool.begin().await?;
    let user_id: i32 = sqlx::query(QUERY)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash)
        .bind(user.role_id)
        .bind(user.two_factor)
        .fetch_one(&mut *transaction)
        .await?
        .get("id");

    if let Some(channel_ids) = user.channel_ids {
        insert_user_channel(&mut transaction, user_id, channel_ids).await?;
    }

    transaction.commit().await?;

    Ok(())
}

pub async fn insert_or_update_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    let password_hash = task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(user.password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await?
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    const QUERY: &str = "INSERT INTO user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5)
            ON CONFLICT(username) DO UPDATE SET
                mail = excluded.mail, username = excluded.username, password = excluded.password, role_id = excluded.role_id, two_factor = excluded.two_factor
        RETURNING id";

    let mut transaction = pool.begin().await?;
    let user_id: i32 = sqlx::query(QUERY)
        .bind(user.mail)
        .bind(user.username)
        .bind(password_hash)
        .bind(user.role_id)
        .bind(user.two_factor)
        .fetch_one(&mut *transaction)
        .await?
        .get("id");

    if let Some(channel_ids) = user.channel_ids {
        sqlx::query("DELETE FROM user_channels WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
        insert_user_channel(&mut transaction, user_id, channel_ids).await?;
    }

    transaction.commit().await?;

    Ok(())
}

pub async fn update_user<'e, E>(
    executor: E,
    id: i32,
    two_factor: Option<bool>,
    mail: Option<String>,
    password_hash: Option<String>,
) -> Result<(), ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
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
    query.build().execute(executor).await?;

    Ok(())
}

pub async fn delete_user(pool: &SqlitePool, id: i32) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM user WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn insert_user_channel(
    executor: &mut SqliteConnection,
    user_id: i32,
    channel_ids: Vec<i32>,
) -> Result<(), ProcessError> {
    for channel in &channel_ids {
        const QUERY: &str =
            "INSERT OR IGNORE INTO user_channels (channel_id, user_id) VALUES ($1, $2);";

        sqlx::query(QUERY)
            .bind(channel)
            .bind(user_id)
            .execute(&mut *executor)
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
    async fn insert_user_rolls_back_when_channel_assignment_fails() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::handles::db_migrate(&pool).await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        let user = User {
            mail: Some("rollback@example.org".to_string()),
            username: "rollback-user".to_string(),
            password: "test-password".to_string(),
            role_id: Some(3),
            channel_ids: Some(vec![i32::MAX]),
            ..User::default()
        };

        assert!(insert_user(&pool, user).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user WHERE username = $1")
            .bind("rollback-user")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
