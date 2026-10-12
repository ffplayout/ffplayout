use sqlx::{SqliteConnection, SqlitePool};

use crate::{
    db::models::{Channel, GlobalSettings, InitialSetup, SetupRollback},
    utils::errors::{ProcessError, ServiceError},
};

pub async fn initialize_setup(pool: &SqlitePool, data: &InitialSetup) -> Result<(), ServiceError> {
    initialize_setup_with_rollback(pool, data).await?;

    Ok(())
}

/// Commit complete database records before starting runtime initialization.
/// A process interruption leaves a configuration that normal startup can load.
pub(crate) async fn initialize_setup_with_rollback(
    pool: &SqlitePool,
    data: &InitialSetup,
) -> Result<SetupRollback, ServiceError> {
    let mut transaction = pool.begin().await?;
    let global: GlobalSettings = sqlx::query_as("SELECT * FROM config_global WHERE id = 1")
        .fetch_one(&mut *transaction)
        .await?;
    let channel: Channel = sqlx::query_as("SELECT * FROM channels WHERE id = 1")
        .fetch_one(&mut *transaction)
        .await?;
    let active_outputs = sqlx::query_scalar(
        "SELECT id FROM config_output WHERE active = 1
         AND config_id = (SELECT id FROM config WHERE channel_id = 1)",
    )
    .fetch_all(&mut *transaction)
    .await?;

    let result = sqlx::query(
        "UPDATE config_global SET logs = $1, playlists = $2, public = $3, storage = $4, shared = $5,
        smtp_server = $6, smtp_user = $7, smtp_password = $8, smtp_starttls = $9, smtp_port = $10,
        setup_completed = 0 WHERE id = 1 AND setup_completed = 0
        AND NOT EXISTS (SELECT 1 FROM auth_user)",
    )
    .bind(&data.settings.logs)
    .bind(&data.settings.playlists)
    .bind(&data.settings.public)
    .bind(&data.settings.storage)
    .bind(data.settings.shared)
    .bind(&data.settings.smtp_server)
    .bind(&data.settings.smtp_user)
    .bind(&data.settings.smtp_password)
    .bind(data.settings.smtp_starttls)
    .bind(data.settings.smtp_port)
    .execute(&mut *transaction)
    .await?;

    if result.rows_affected() != 1 {
        return Err(ServiceError::Conflict(
            "Installation has already been initialized".to_string(),
        ));
    }

    sqlx::query("UPDATE channels SET public = $1, playlists = $2, storage = $3 WHERE id = 1")
        .bind(&data.channel_public)
        .bind(&data.channel_playlists)
        .bind(&data.channel_storage)
        .execute(&mut *transaction)
        .await?;

    sqlx::query(
        "UPDATE config_output SET active = 0
         WHERE config_id = (SELECT id FROM config WHERE channel_id = 1)",
    )
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "UPDATE config_output SET active = 1
         WHERE config_id = (SELECT id FROM config WHERE channel_id = 1) AND name = 'hls'",
    )
    .execute(&mut *transaction)
    .await?;

    let user_id: i32 = sqlx::query_scalar(
        "INSERT INTO auth_user (mail, username, password, role_id, two_factor)
        VALUES ($1, $2, $3, 1, $4) RETURNING id",
    )
    .bind(data.mail.trim())
    .bind(data.username.trim())
    .bind(&data.password_hash)
    .bind(data.two_factor)
    .fetch_one(&mut *transaction)
    .await?;

    sqlx::query("INSERT INTO auth_user_channels (channel_id, user_id) VALUES (1, $1)")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;

    sqlx::query("UPDATE config_global SET setup_completed = 1 WHERE id = 1")
        .execute(&mut *transaction)
        .await?;

    let snapshot = SetupRollback {
        global,
        channel,
        active_outputs,
        user_id,
    };

    transaction.commit().await?;

    Ok(snapshot)
}

pub(crate) async fn rollback_setup(
    pool: &SqlitePool,
    snapshot: SetupRollback,
) -> Result<(), ServiceError> {
    let mut transaction = pool.begin().await?;
    restore_setup(&mut transaction, snapshot).await?;
    transaction.commit().await?;

    Ok(())
}

async fn restore_setup(
    connection: &mut SqliteConnection,
    snapshot: SetupRollback,
) -> Result<(), ProcessError> {
    sqlx::query("DELETE FROM auth_user WHERE id = $1")
        .bind(snapshot.user_id)
        .execute(&mut *connection)
        .await?;
    let global = snapshot.global;
    sqlx::query(
        "UPDATE config_global SET logs = $1, playlists = $2, public = $3, storage = $4,
         shared = $5, smtp_server = $6, smtp_user = $7, smtp_password = $8,
         smtp_starttls = $9, smtp_port = $10, setup_completed = $11 WHERE id = 1",
    )
    .bind(global.logs)
    .bind(global.playlists)
    .bind(global.public)
    .bind(global.storage)
    .bind(global.shared)
    .bind(global.smtp_server)
    .bind(global.smtp_user)
    .bind(global.smtp_password)
    .bind(global.smtp_starttls)
    .bind(global.smtp_port)
    .bind(global.setup_completed)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE channels SET public = $1, playlists = $2, storage = $3, active = $4 WHERE id = 1",
    )
    .bind(snapshot.channel.public)
    .bind(snapshot.channel.playlists)
    .bind(snapshot.channel.storage)
    .bind(snapshot.channel.active)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE config_output SET active = 0
         WHERE config_id = (SELECT id FROM config WHERE channel_id = 1)",
    )
    .execute(&mut *connection)
    .await?;

    for id in snapshot.active_outputs {
        sqlx::query("UPDATE config_output SET active = 1 WHERE id = $1")
            .bind(id)
            .execute(&mut *connection)
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use crate::db::{handles, models::SetupSettings};

    use super::*;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();

        pool
    }

    fn setup_data() -> InitialSetup {
        InitialSetup {
            settings: SetupSettings {
                logs: "/test/logs".to_string(),
                playlists: "/test/playlists".to_string(),
                public: "/test/public".to_string(),
                storage: "/test/storage".to_string(),
                shared: true,
                smtp_server: "smtp.example.org".to_string(),
                smtp_user: "test".to_string(),
                smtp_password: "test".to_string(),
                smtp_starttls: true,
                smtp_port: 587,
            },
            channel_public: "/test/public/1".to_string(),
            channel_playlists: "/test/playlists/1".to_string(),
            channel_storage: "/test/storage/1".to_string(),
            username: " setup-admin ".to_string(),
            mail: " setup@example.org ".to_string(),
            password_hash: "already-hashed".to_string(),
            two_factor: false,
        }
    }

    #[tokio::test]
    async fn interrupted_runtime_initialization_leaves_complete_setup_on_disk() {
        let path =
            std::env::temp_dir().join(format!("ffplayout-setup-{}.db", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        let snapshot = initialize_setup_with_rollback(&pool, &setup_data())
            .await
            .unwrap();

        // Simulate exiting after the commit, before runtime initialization.
        drop(snapshot);
        pool.close().await;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        let global = handles::select_global(&pool).await.unwrap();
        let user = handles::select_login(&pool, "setup-admin").await.unwrap();
        let channel = handles::select_channel(&pool, &1).await.unwrap();
        let outputs: Vec<String> =
            sqlx::query_scalar("SELECT name FROM config_output WHERE active = 1")
                .fetch_all(&pool)
                .await
                .unwrap();

        assert!(global.setup_completed);
        assert_eq!(user.role_id, Some(1));
        assert_eq!(user.channel_ids, Some(vec![1]));
        assert_eq!(channel.storage, setup_data().channel_storage);
        assert_eq!(outputs, ["hls"]);
        assert!(matches!(
            initialize_setup(&pool, &setup_data()).await,
            Err(ServiceError::Conflict(_))
        ));
        pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn setup_initializes_once_and_preserves_existing_data_on_retry() {
        let pool = setup_pool().await;
        let mut data = setup_data();
        initialize_setup(&pool, &data).await.unwrap();
        let global = handles::select_global(&pool).await.unwrap();
        let channel = handles::select_channel(&pool, &1).await.unwrap();
        let user = handles::select_login(&pool, "setup-admin").await.unwrap();
        let active_outputs: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM config_output WHERE active = 1 AND config_id = (SELECT id FROM config WHERE channel_id = 1)",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(global.setup_completed);
        assert_eq!(global.logs, data.settings.logs);
        assert_eq!(channel.public, data.channel_public);
        assert_eq!(channel.playlists, data.channel_playlists);
        assert_eq!(channel.storage, data.channel_storage);
        assert_eq!(user.mail.as_deref(), Some("setup@example.org"));
        assert_eq!(user.password, data.password_hash);
        assert_eq!(user.role_id, Some(1));
        assert_eq!(user.channel_ids, Some(vec![1]));
        assert!(!user.two_factor);
        assert_eq!(active_outputs, ["hls"]);
        data.settings.logs = "/different/logs".to_string();

        assert!(matches!(
            initialize_setup(&pool, &data).await,
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(handles::count_users(&pool).await.unwrap(), 1);
        assert_eq!(
            handles::select_global(&pool).await.unwrap().logs,
            global.logs
        );
    }

    #[tokio::test]
    async fn setup_rolls_back_settings_channels_and_users_when_assignment_fails() {
        let pool = setup_pool().await;
        let global_before = handles::select_global(&pool).await.unwrap();
        let channel_before = handles::select_channel(&pool, &1).await.unwrap();
        sqlx::query("CREATE TRIGGER reject_setup_assignment BEFORE INSERT ON auth_user_channels BEGIN SELECT RAISE(ABORT, 'forced assignment failure'); END")
            .execute(&pool)
            .await
            .unwrap();

        assert!(initialize_setup(&pool, &setup_data()).await.is_err());
        let global_after = handles::select_global(&pool).await.unwrap();
        let channel_after = handles::select_channel(&pool, &1).await.unwrap();

        assert!(!global_after.setup_completed);
        assert_eq!(global_after.logs, global_before.logs);
        assert_eq!(channel_after.public, channel_before.public);
        assert_eq!(channel_after.storage, channel_before.storage);
        assert_eq!(handles::count_users(&pool).await.unwrap(), 0);
    }
}
