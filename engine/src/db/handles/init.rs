use rand::{RngExt, distr::Alphanumeric};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::handles::select_global,
    utils::{errors::ProcessError, is_running_in_container},
};

pub async fn db_migrate(pool: &SqlitePool) -> Result<bool, ProcessError> {
    sqlx::migrate!("../migrations").run(pool).await?;
    let mut init = false;

    if select_global(pool).await.is_err() {
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
            .execute(pool)
            .await?;

        init = true;
    }

    Ok(init)
}
