use rand::{RngExt, distr::Alphanumeric};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::handles::select_global,
    utils::{errors::ProcessError, is_running_in_container},
};

pub async fn db_migrate(pool: &SqlitePool) -> Result<bool, ProcessError> {
    let existing_schema: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name IN ('global', 'config_global'))",
    )
    .fetch_one(pool)
    .await?;
    sqlx::migrate!("../../migrations").run(pool).await?;

    if !existing_schema {
        // Earlier migrations seed the initial channel. It is a new configuration,
        // not an existing installation whose legacy selection must be preserved.
        sqlx::query("UPDATE config_audio SET loudness_scope = 'all', loudness_enable = 1")
            .execute(pool)
            .await?;
    }
    let mut init = false;

    if select_global(pool).await.is_err() {
        let secret: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(80)
            .map(char::from)
            .collect();
        let shared = is_running_in_container();

        const QUERY: &str = "CREATE TRIGGER config_global_row_count
        BEFORE INSERT ON config_global
        WHEN (SELECT COUNT(*) FROM config_global) >= 1
        BEGIN
            SELECT RAISE(FAIL, 'Database is already initialized!');
        END;
        INSERT INTO config_global(secret, shared, setup_completed) VALUES($1, $2, 0);";

        sqlx::query(QUERY)
            .bind(secret)
            .bind(shared)
            .execute(pool)
            .await?;

        init = true;
    }

    Ok(init)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[tokio::test]
    async fn fresh_installation_normalizes_all_sources_and_keeps_user_selection_on_restart() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let scope: String =
            sqlx::query_scalar("SELECT loudness_scope FROM config_audio WHERE config_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(scope, "all");
        sqlx::query("UPDATE config_audio SET loudness_scope = 'off' WHERE config_id = 1")
            .execute(&pool)
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let scope: String =
            sqlx::query_scalar("SELECT loudness_scope FROM config_audio WHERE config_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(scope, "off");
    }
}
