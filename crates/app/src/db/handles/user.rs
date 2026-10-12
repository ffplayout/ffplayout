use argon2::{Argon2, PasswordHasher};
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
    const QUERY: &str = "SELECT name FROM auth_roles WHERE id = $1";
    let result: Role = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_login(pool: &SqlitePool, user: &str) -> Result<User, ProcessError> {
    const QUERY: &str =
        "SELECT u.id, u.mail, u.username, u.password, u.role_id, u.two_factor, group_concat(uc.channel_id, ',') as channel_ids FROM auth_user u
        left join auth_user_channels uc on uc.user_id = u.id
    WHERE u.username = $1
    GROUP BY u.id";

    let result = sqlx::query_as(QUERY).bind(user).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_user(pool: &SqlitePool, id: i32) -> Result<User, ProcessError> {
    const QUERY: &str = "SELECT u.id, u.mail, u.username, u.role_id, u.two_factor, group_concat(uc.channel_id, ',') as channel_ids FROM auth_user u
        left join auth_user_channels uc on uc.user_id = u.id
    WHERE u.id = $1
    GROUP BY u.id";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_users(pool: &SqlitePool) -> Result<Vec<User>, ProcessError> {
    const QUERY: &str = "SELECT id, username FROM auth_user";

    let result = sqlx::query_as(QUERY).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_user(pool: &SqlitePool, user: User) -> Result<(), ServiceError> {
    const QUERY: &str = "INSERT INTO auth_user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5) RETURNING id";

    let password_hash = task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(user.password.as_bytes())
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
        Argon2::default()
            .hash_password(user.password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await?
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    const QUERY: &str = "INSERT INTO auth_user (mail, username, password, role_id, two_factor) VALUES($1, $2, $3, $4, $5)
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
        delete_user_channels(&mut *transaction, user_id).await?;
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

    let mut query = QueryBuilder::<Sqlite>::new("UPDATE auth_user SET ");
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

pub async fn update_user_with_channels(
    pool: &SqlitePool,
    id: i32,
    two_factor: Option<bool>,
    mail: Option<String>,
    password_hash: Option<String>,
    channel_ids: Option<Vec<i32>>,
) -> Result<(), ProcessError> {
    let mut transaction = pool.begin().await?;
    update_user(&mut *transaction, id, two_factor, mail, password_hash).await?;

    if let Some(channel_ids) = channel_ids {
        delete_user_channels(&mut *transaction, id).await?;
        insert_user_channel(&mut transaction, id, channel_ids).await?;
    }

    transaction.commit().await?;

    Ok(())
}

pub async fn delete_user(pool: &SqlitePool, id: i32) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM auth_user WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn count_users(pool: &SqlitePool) -> Result<i64, ProcessError> {
    let count = sqlx::query_scalar("SELECT COUNT(*) FROM auth_user")
        .fetch_one(pool)
        .await?;

    Ok(count)
}

pub async fn delete_user_channels<'e, E>(executor: E, user_id: i32) -> Result<(), ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("DELETE FROM auth_user_channels WHERE user_id = $1")
        .bind(user_id)
        .execute(executor)
        .await?;

    Ok(())
}

pub async fn map_global_admins(pool: &SqlitePool) -> Result<(), ProcessError> {
    sqlx::query(
        "INSERT OR IGNORE INTO auth_user_channels (channel_id, user_id)
         SELECT channels.id, auth_user.id FROM channels CROSS JOIN auth_user WHERE auth_user.role_id = 1",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn assign_channel_to_global_admins<'e, E>(
    executor: E,
    channel_id: i32,
) -> Result<(), ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT OR IGNORE INTO auth_user_channels (channel_id, user_id)
         SELECT $1, id FROM auth_user WHERE role_id = 1",
    )
    .bind(channel_id)
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn insert_user_channel(
    executor: &mut SqliteConnection,
    user_id: i32,
    channel_ids: Vec<i32>,
) -> Result<(), ProcessError> {
    for channel in &channel_ids {
        const QUERY: &str =
            "INSERT OR IGNORE INTO auth_user_channels (channel_id, user_id) VALUES ($1, $2);";

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
        sqlx::query("CREATE TABLE auth_user (id INTEGER PRIMARY KEY, two_factor INTEGER, mail TEXT, password TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO auth_user (id, two_factor) VALUES (1, 1)")
            .execute(&pool)
            .await
            .unwrap();

        update_user(&pool, 1, Some(false), None, None)
            .await
            .unwrap();

        let two_factor: i32 = sqlx::query_scalar("SELECT two_factor FROM auth_user WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(two_factor, 0);
    }

    #[tokio::test]
    async fn updating_user_and_channels_is_atomic() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::handles::db_migrate(&pool).await.unwrap();
        sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id) VALUES (1, 'before@example.org', 'update-user', 'old-hash', 3)")
            .execute(&pool)
            .await
            .unwrap();
        insert_user_channel(&mut pool.acquire().await.unwrap(), 1, vec![1])
            .await
            .unwrap();
        let result = update_user_with_channels(
            &pool,
            1,
            None,
            Some("after@example.org".to_string()),
            Some("new-hash".to_string()),
            Some(vec![i32::MAX]),
        )
        .await;

        assert!(result.is_err());
        let unchanged = select_login(&pool, "update-user").await.unwrap();

        assert_eq!(unchanged.mail.as_deref(), Some("before@example.org"));
        assert_eq!(unchanged.password, "old-hash");
        assert_eq!(unchanged.channel_ids, Some(vec![1]));
        update_user_with_channels(
            &pool,
            1,
            None,
            Some("after@example.org".to_string()),
            None,
            Some(Vec::new()),
        )
        .await
        .unwrap();
        let updated = select_login(&pool, "update-user").await.unwrap();

        assert_eq!(updated.mail.as_deref(), Some("after@example.org"));
        assert!(!updated.channel_ids.unwrap().contains(&1));
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
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_user WHERE username = $1")
            .bind("rollback-user")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
