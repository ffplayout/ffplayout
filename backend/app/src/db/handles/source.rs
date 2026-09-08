use sqlx::{SqliteConnection, sqlite::SqliteQueryResult};

use crate::utils::errors::ProcessError;

/// Creates the per-channel source-options row with safe empty defaults.
///
/// The options are persisted ahead of their future engine integration so each
/// configuration always has a stable place for demuxer and input-protocol
/// settings.
pub async fn insert_source(
    connection: &mut SqliteConnection,
    config_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    Ok(
        sqlx::query("INSERT INTO config_source (config_id) VALUES ($1)")
            .bind(config_id)
            .execute(connection)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::{db_migrate, insert_channel},
        models::Channel,
    };

    #[tokio::test]
    async fn inserts_empty_source_options() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let channel = insert_channel(
            &pool,
            Channel {
                name: "input-options-test".to_string(),
                preview_url: String::new(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();
        let config_id: i32 =
            sqlx::query_scalar("INSERT INTO config (channel_id) VALUES ($1) RETURNING id")
                .bind(channel.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut connection = pool.acquire().await.unwrap();

        insert_source(&mut connection, config_id).await.unwrap();

        let options: (String, String) = sqlx::query_as(
            "SELECT demuxer_options, protocol_options FROM config_source WHERE config_id = $1",
        )
        .bind(config_id)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(options, ("{}".to_string(), "{}".to_string()));
    }
}
