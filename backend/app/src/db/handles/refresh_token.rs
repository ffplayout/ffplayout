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
    sqlx::query("DELETE FROM refresh_tokens WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO refresh_tokens
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
    sqlx::query("DELETE FROM refresh_tokens WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    let token = sqlx::query(
        "SELECT family_id, expires_at, revoked_at
         FROM refresh_tokens WHERE jti = $1 AND user_id = $2",
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
            "UPDATE refresh_tokens SET revoked_at = $1
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
        "UPDATE refresh_tokens SET revoked_at = $1, replaced_by = $2
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
            "UPDATE refresh_tokens SET revoked_at = $1
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
        "INSERT INTO refresh_tokens
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
        "UPDATE refresh_tokens SET revoked_at = $1
         WHERE family_id = (
             SELECT family_id FROM refresh_tokens WHERE jti = $2 AND user_id = $3
         ) AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(jti)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
