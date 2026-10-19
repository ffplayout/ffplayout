use sqlx::{Row, sqlite::SqlitePool};

use crate::utils::errors::ProcessError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshRotation {
    Rotated,
    Invalid,
    Reused,
}

pub async fn insert_refresh_token(
    pool: &SqlitePool,
    jti: &str,
    family_id: &str,
    user_id: i32,
    expires_at: i64,
    now: i64,
) -> Result<(), ProcessError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM auth_refresh_tokens WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO auth_refresh_tokens
         (jti, family_id, user_id, expires_at, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(jti)
    .bind(family_id)
    .bind(user_id)
    .bind(expires_at)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(())
}

pub async fn rotate_refresh_token(
    pool: &SqlitePool,
    old_jti: &str,
    new_jti: &str,
    user_id: i32,
    expires_at: i64,
    now: i64,
) -> Result<RefreshRotation, ProcessError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM auth_refresh_tokens WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    let token = sqlx::query(
        "SELECT family_id, expires_at, revoked_at
         FROM auth_refresh_tokens WHERE jti = $1 AND user_id = $2",
    )
    .bind(old_jti)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(token) = token else {
        return Ok(RefreshRotation::Invalid);
    };
    let family_id: String = token.get("family_id");
    let stored_expires_at: i64 = token.get("expires_at");
    let revoked_at: Option<i64> = token.get("revoked_at");

    if revoked_at.is_some() {
        sqlx::query(
            "UPDATE auth_refresh_tokens SET revoked_at = $1
             WHERE family_id = $2 AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(&family_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(RefreshRotation::Reused);
    }
    if stored_expires_at <= now {
        return Ok(RefreshRotation::Invalid);
    }

    let result = sqlx::query(
        "UPDATE auth_refresh_tokens SET revoked_at = $1, replaced_by = $2
         WHERE jti = $3 AND user_id = $4 AND revoked_at IS NULL AND expires_at > $1",
    )
    .bind(now)
    .bind(new_jti)
    .bind(old_jti)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() != 1 {
        sqlx::query(
            "UPDATE auth_refresh_tokens SET revoked_at = $1
             WHERE family_id = $2 AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(&family_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(RefreshRotation::Reused);
    }

    sqlx::query(
        "INSERT INTO auth_refresh_tokens
         (jti, family_id, user_id, expires_at, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(new_jti)
    .bind(family_id)
    .bind(user_id)
    .bind(expires_at)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(RefreshRotation::Rotated)
}

pub async fn revoke_refresh_family(
    pool: &SqlitePool,
    jti: &str,
    user_id: i32,
    now: i64,
) -> Result<bool, ProcessError> {
    let result = sqlx::query(
        "UPDATE auth_refresh_tokens SET revoked_at = $1
         WHERE family_id = (
             SELECT family_id FROM auth_refresh_tokens WHERE jti = $2 AND user_id = $3
         ) AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(jti)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use tokio::sync::Barrier;

    use crate::db::handles;

    use super::*;

    async fn populate(pool: &SqlitePool) {
        handles::db_migrate(pool).await.unwrap();
        sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id) VALUES (1, 'refresh@example.org', 'refresh-user', 'unused', 3)")
            .execute(pool)
            .await
            .unwrap();
        insert_refresh_token(pool, "original", "family", 1, 1000, 100)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_rotation_has_one_winner_and_revokes_its_successor_on_reuse() {
        let path =
            std::env::temp_dir().join(format!("ffplayout-refresh-{}.db", uuid::Uuid::new_v4()));
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(Duration::from_secs(5)),
            )
            .await
            .unwrap();
        populate(&pool).await;
        let barrier = Arc::new(Barrier::new(2));
        let rotate = |new_jti: &'static str| {
            let pool = pool.clone();
            let barrier = barrier.clone();

            tokio::spawn(async move {
                barrier.wait().await;

                rotate_refresh_token(&pool, "original", new_jti, 1, 1000, 200)
                    .await
                    .unwrap()
            })
        };
        let first = rotate("first");
        let second = rotate("second");
        let outcomes = [first.await.unwrap(), second.await.unwrap()];

        assert_eq!(
            outcomes
                .iter()
                .filter(|result| **result == RefreshRotation::Rotated)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| **result == RefreshRotation::Reused)
                .count(),
            1
        );
        let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_refresh_tokens WHERE family_id = 'family' AND revoked_at IS NULL")
            .fetch_one(&pool).await.unwrap();

        assert_eq!(live, 0);
        pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn failed_successor_insert_rolls_back_rotation_and_allows_retry() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        populate(&pool).await;
        insert_refresh_token(&pool, "existing", "other-family", 1, 1000, 100)
            .await
            .unwrap();

        assert!(
            rotate_refresh_token(&pool, "original", "existing", 1, 1000, 200)
                .await
                .is_err()
        );
        let original: (Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT revoked_at, replaced_by FROM auth_refresh_tokens WHERE jti = 'original'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(original, (None, None));
        assert_eq!(
            rotate_refresh_token(&pool, "original", "retry", 1, 1000, 200)
                .await
                .unwrap(),
            RefreshRotation::Rotated
        );
    }

    #[tokio::test]
    async fn invalid_expired_and_wrong_user_tokens_cannot_rotate() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        populate(&pool).await;

        assert_eq!(
            rotate_refresh_token(&pool, "missing", "new", 1, 1000, 200)
                .await
                .unwrap(),
            RefreshRotation::Invalid
        );
        assert_eq!(
            rotate_refresh_token(&pool, "original", "new", 2, 1000, 200)
                .await
                .unwrap(),
            RefreshRotation::Invalid
        );
        assert_eq!(
            rotate_refresh_token(&pool, "original", "new", 1, 2000, 1000)
                .await
                .unwrap(),
            RefreshRotation::Invalid
        );
        let successors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM auth_refresh_tokens WHERE jti = 'new'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(successors, 0);
    }
}
