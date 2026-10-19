use std::collections::HashSet;

use sqlx::{
    Executor, Sqlite,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{
    db::{
        handles,
        models::{self, Channel},
    },
    utils::{
        config::{DEFAULT_INGEST_PORT, OutputMode, parse_rtmp_ingest_port},
        errors::{ProcessError, ServiceError},
    },
};

pub async fn select_channel(pool: &SqlitePool, id: &i32) -> Result<Channel, ProcessError> {
    const QUERY: &str = "SELECT * FROM channels WHERE id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_related_channels(
    pool: &SqlitePool,
    user_id: Option<i32>,
) -> Result<Vec<Channel>, ProcessError> {
    let result = match user_id {
        Some(id) => {
            const QUERY: &str =
                "SELECT c.id, c.name, c.preview_url, c.extra_extensions, c.active, c.public, c.playlists,
            c.storage, c.last_date, c.time_shift, c.timezone FROM channels c
                left join auth_user_channels uc on uc.channel_id = c.id
                left join auth_user u on u.id = uc.user_id
             WHERE u.id = $1 ORDER BY c.id ASC;";

            sqlx::query_as(QUERY).bind(id).fetch_all(pool).await?
        }
        None => {
            const QUERY: &str = "SELECT * FROM channels ORDER BY id ASC;";

            sqlx::query_as(QUERY).fetch_all(pool).await?
        }
    };

    Ok(result)
}

pub async fn update_channel(
    pool: &SqlitePool,
    id: i32,
    channel: Channel,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE channels SET name = $2, preview_url = $3, extra_extensions = $4, public = $5, playlists = $6, storage = $7, timezone = $8 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .bind(channel.timezone.map(|tz| tz.to_string()))
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn insert_channel<'e, E>(executor: E, channel: Channel) -> Result<Channel, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    const QUERY: &str = "INSERT INTO channels (name, preview_url, extra_extensions, public, playlists, storage) VALUES($1, $2, $3, $4, $5, $6) RETURNING *";
    let result = sqlx::query_as::<_, Channel>(QUERY)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .fetch_one(executor)
        .await?;

    Ok(result)
}

pub async fn delete_channel(
    pool: &SqlitePool,
    id: &i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM channels WHERE id = $1";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn update_stat(
    pool: &SqlitePool,
    id: i32,
    last_date: &Option<String>,
    time_shift: f64,
) -> Result<SqliteQueryResult, ProcessError> {
    let query = match last_date {
        Some(_) => "UPDATE channels SET last_date = $2, time_shift = $3 WHERE id = $1",
        None => "UPDATE channels SET time_shift = $2 WHERE id = $1",
    };

    let mut q = sqlx::query(query).bind(id);

    if last_date.is_some() {
        q = q.bind(last_date);
    }

    let result = q.bind(time_shift).execute(pool).await?;

    Ok(result)
}

pub async fn update_player(
    pool: &SqlitePool,
    id: i32,
    active: bool,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE channels SET active = $2 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(active)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn create_channel_records(
    conn: &SqlitePool,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    let mut transaction = conn.begin().await?;
    let configured_ingest_urls = handles::select_rtmp_ingest_urls(&mut *transaction).await?;
    let ingest_url = default_ingest_url(next_available_ingest_port(&configured_ingest_urls)?);
    let channel = handles::insert_channel(&mut *transaction, target_channel).await?;
    let outputs = [
        models::Output::new(channel.id, OutputMode::HLS),
        models::Output::new(channel.id, OutputMode::Stream),
        models::Output::new(channel.id, OutputMode::Desktop),
    ];

    let config_id =
        handles::insert_configuration(&mut transaction, channel.id, &ingest_url).await?;
    handles::insert_source(&mut transaction, config_id).await?;
    handles::new_channel_presets(&mut *transaction, config_id).await?;

    for (index, output) in outputs.iter().enumerate() {
        handles::insert_output(&mut *transaction, config_id, output, index == 0).await?;
    }

    handles::insert_recording(&mut transaction, config_id, channel.id).await?;
    handles::assign_channel_to_global_admins(&mut *transaction, channel.id).await?;
    transaction.commit().await?;

    Ok(channel)
}

fn default_ingest_url(port: u16) -> String {
    format!("rtmp://127.0.0.1:{port}/live/stream")
}

fn next_available_ingest_port(configured_urls: &[String]) -> Result<u16, ServiceError> {
    let used_ports = configured_urls
        .iter()
        .filter_map(|url| parse_rtmp_ingest_port(url).ok())
        .collect::<HashSet<_>>();

    (DEFAULT_INGEST_PORT..=u16::MAX)
        .find(|port| !used_ports.contains(port))
        .ok_or_else(|| {
            ServiceError::BadRequest("no free unprivileged ingest port available".to_string())
        })
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[tokio::test]
    async fn channel_creation_rolls_back_all_records_on_failure() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        sqlx::query(
            "CREATE TRIGGER reject_test_configuration
             BEFORE INSERT ON config WHEN NEW.channel_id > 1
             BEGIN SELECT RAISE(FAIL, 'forced failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
            .fetch_one(&pool)
            .await
            .unwrap();

        let result = create_channel_records(
            &pool,
            Channel {
                name: "rollback-test".to_string(),
                ..Channel::default()
            },
        )
        .await;

        assert!(result.is_err());
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, before);
        let orphan_outputs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM config_output WHERE config_id NOT IN (SELECT id FROM config)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphan_outputs, 0);
    }

    #[tokio::test]
    async fn channel_creation_assigns_the_channel_to_every_global_admin() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO auth_user (mail, username, password, role_id, two_factor) VALUES
             ('one@example.org', 'admin-one', 'hash', 1, 0),
             ('two@example.org', 'admin-two', 'hash', 1, 0),
             ('user@example.org', 'regular-user', 'hash', 3, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channel = create_channel_records(
            &pool,
            Channel {
                name: "admin-mapping-test".to_string(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();
        let assigned_roles: Vec<i32> = sqlx::query_scalar(
            "SELECT auth_user.role_id FROM auth_user_channels
             JOIN auth_user ON auth_user.id = auth_user_channels.user_id
             WHERE auth_user_channels.channel_id = $1 ORDER BY auth_user.role_id",
        )
        .bind(channel.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(assigned_roles, [1, 1]);
    }

    #[tokio::test]
    async fn channel_creation_assigns_the_next_free_ingest_port() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();

        let second = create_channel_records(
            &pool,
            Channel {
                name: "second-channel".to_string(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();
        let third = create_channel_records(
            &pool,
            Channel {
                name: "third-channel".to_string(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();

        let second_url: String = sqlx::query_scalar(
            "SELECT ingest.identifier FROM config
                JOIN config_live_input ingest ON ingest.config_id = config.id
                WHERE ingest.backend = 'rtmp' AND ingest.takeover_mode = 'connection'
                  AND config.channel_id = $1",
        )
        .bind(second.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let third_url: String = sqlx::query_scalar(
            "SELECT ingest.identifier FROM config
                JOIN config_live_input ingest ON ingest.config_id = config.id
                WHERE ingest.backend = 'rtmp' AND ingest.takeover_mode = 'connection'
                  AND config.channel_id = $1",
        )
        .bind(third.id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(second_url, "rtmp://127.0.0.1:1937/live/stream");
        assert_eq!(third_url, "rtmp://127.0.0.1:1938/live/stream");
        sqlx::query(
            "UPDATE config_live_input SET enabled = 1 WHERE backend = 'rtmp'
             AND takeover_mode = 'connection'
             AND config_id IN (SELECT id FROM config WHERE channel_id IN (1, $1))",
        )
        .bind(second.id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            handles::ingest_port_in_use(&pool, third.id, 1936)
                .await
                .unwrap()
        );
        assert!(
            handles::ingest_port_in_use(&pool, third.id, 1937)
                .await
                .unwrap()
        );
        assert!(
            !handles::ingest_port_in_use(&pool, third.id, 1938)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn deleting_a_channel_cascades_to_every_config_table() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        let channel = create_channel_records(
            &pool,
            Channel {
                name: "cascade-test".to_string(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();
        let config_id: i32 = sqlx::query_scalar("SELECT id FROM config WHERE channel_id = $1")
            .bind(channel.id)
            .fetch_one(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO config_live_input (config_id, priority, backend) VALUES ($1, 50, 'ndi')",
        )
        .bind(config_id)
        .execute(&pool)
        .await
        .unwrap();

        handles::delete_channel(&pool, &channel.id).await.unwrap();

        for table in [
            "config",
            "config_general",
            "config_mail",
            "config_notification",
            "config_logging",
            "config_processing",
            "config_audio",
            "config_source",
            "config_live_input",
            "config_playlist",
            "config_storage",
            "config_task",
            "config_output",
            "config_recording",
            "config_text_presets",
        ] {
            let query = format!(
                "SELECT COUNT(*) FROM {table} WHERE {} = $1",
                if table == "config" { "id" } else { "config_id" }
            );
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
                .bind(config_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 0, "orphaned row in {table}");
        }
    }
}
