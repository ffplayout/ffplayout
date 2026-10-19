use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use argon2::{Argon2, PasswordHasher};
use log::warn;
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::{
    sync::{Mutex, RwLock},
    task,
};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{
        handles,
        models::{InitialSetup, SetupSettings},
    },
    file::init_storage,
    player::controller::ChannelController,
    utils::{
        channels::initialize_channels, errors::ServiceError, mail::MailQueue,
        paths::validate_directory_path, system::SystemStat,
    },
};

static SETUP_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    #[serde(flatten)]
    pub settings: SetupSettings,
    pub username: String,
    pub mail: String,
    pub password: String,
    pub two_factor: bool,
}

fn validate_setup_paths(settings: &SetupSettings) -> Result<(), ServiceError> {
    [
        ("Logging", settings.logs.as_str()),
        ("Playlist", settings.playlists.as_str()),
        ("Public", settings.public.as_str()),
        ("Storage", settings.storage.as_str()),
    ]
    .into_iter()
    .try_for_each(|(name, path)| {
        validate_directory_path(name, path).map_err(ServiceError::BadRequest)
    })
}

pub async fn complete_setup(
    pool: &SqlitePool,
    controllers: Arc<RwLock<ChannelController>>,
    mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    shutdown: CancellationToken,
    system: SystemStat,
    data: SetupRequest,
) -> Result<(), ServiceError> {
    if data.username.trim().is_empty() || data.mail.trim().is_empty() || data.password.is_empty() {
        return Err(ServiceError::BadRequest(
            "Username, email, and password are required".to_string(),
        ));
    }

    validate_setup_paths(&data.settings)?;

    let _setup_guard = SETUP_LOCK.lock().await;
    let global = handles::select_global(pool).await?;

    if global.setup_completed || handles::count_users(pool).await? != 0 {
        return Err(ServiceError::Conflict(
            "Installation has already been initialized".to_string(),
        ));
    }

    let password = data.password;
    let password_hash = task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await?
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    let settings = data.settings;
    let channel_path = |path: &str| {
        if settings.shared {
            Path::new(path).join("1").to_string_lossy().to_string()
        } else {
            path.to_string()
        }
    };
    let paths = [
        PathBuf::from(&settings.logs),
        PathBuf::from(channel_path(&settings.public)),
        PathBuf::from(channel_path(&settings.playlists)),
        PathBuf::from(channel_path(&settings.storage)),
    ];

    for path in paths {
        tokio::fs::create_dir_all(&path).await.map_err(|error| {
            ServiceError::Conflict(format!("Cannot create {}: {error}", path.display()))
        })?;
    }

    let setup = InitialSetup {
        channel_public: channel_path(&settings.public),
        channel_playlists: channel_path(&settings.playlists),
        channel_storage: channel_path(&settings.storage),
        settings,
        username: data.username,
        mail: data.mail,
        password_hash,
        two_factor: data.two_factor,
    };
    let storage = init_storage(PathBuf::from(&setup.channel_storage), Vec::new()).await?;

    if let Err(error) = storage.copy_assets().await {
        warn!("Could not copy initial storage assets: {error}");
    }

    let snapshot = handles::initialize_setup_with_rollback(pool, &setup).await?;
    let pending_controllers = Arc::new(RwLock::new(ChannelController::new()));
    let pending_queues = Arc::new(Mutex::new(Vec::new()));
    let pending_shutdown = shutdown.child_token();
    let result = initialize_channels(
        pool,
        pending_controllers.clone(),
        pending_queues.clone(),
        pending_shutdown.clone(),
        system,
        false,
    )
    .await;

    if let Err(error) = result {
        pending_shutdown.cancel();
        let managers = pending_controllers.read().await.managers.clone();

        for manager in managers {
            manager.stop_all(false).await;
            manager.stop_supervisor().await;
        }

        handles::rollback_setup(pool, snapshot)
            .await
            .map_err(|rollback_error| {
                ServiceError::Conflict(format!(
                    "Setup failed: {error}; rollback failed: {rollback_error}"
                ))
            })?;

        return Err(error);
    }

    let managers = std::mem::take(&mut pending_controllers.write().await.managers);
    controllers.write().await.managers.extend(managers);
    let queues = std::mem::take(&mut *pending_queues.lock().await);
    mail_queues.lock().await.extend(queues);

    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use crate::utils::paths::validate_directory_path as validate_setup_path;

    use super::*;

    fn request(root: &Path) -> SetupRequest {
        let directory = |name: &str| root.join(name).to_string_lossy().into_owned();

        SetupRequest {
            settings: SetupSettings {
                logs: directory("logs"),
                playlists: directory("playlists"),
                public: directory("public"),
                storage: directory("storage"),
                shared: false,
                smtp_server: String::new(),
                smtp_user: String::new(),
                smtp_password: String::new(),
                smtp_starttls: false,
                smtp_port: 465,
            },
            username: "setup-admin".to_string(),
            mail: "setup@example.org".to_string(),
            password: "setup-password".to_string(),
            two_factor: false,
        }
    }

    async fn setup_state() -> (
        SqlitePool,
        Arc<RwLock<ChannelController>>,
        Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    ) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();

        (
            pool,
            Arc::new(RwLock::new(ChannelController::new())),
            Arc::new(Mutex::new(Vec::new())),
        )
    }

    fn root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("setup-test-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn filesystem_failure_leaves_database_and_runtime_uninitialized() {
        let (pool, controllers, queues) = setup_state().await;
        let root = root();
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("logs"), b"not a directory")
            .await
            .unwrap();
        let global_before = handles::select_global(&pool).await.unwrap();
        let result = complete_setup(
            &pool,
            controllers.clone(),
            queues.clone(),
            CancellationToken::new(),
            SystemStat::new(),
            request(&root),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(handles::count_users(&pool).await.unwrap(), 0);
        let global = handles::select_global(&pool).await.unwrap();

        assert!(!global.setup_completed);
        assert_eq!(global.logs, global_before.logs);
        assert!(controllers.read().await.managers.is_empty());
        assert!(queues.lock().await.is_empty());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn failed_channel_initialization_rolls_back_setup_and_can_be_retried() {
        let (pool, controllers, queues) = setup_state().await;
        let root = root();
        let global_before = handles::select_global(&pool).await.unwrap();
        let channel_before = handles::select_channel(&pool, &1).await.unwrap();
        sqlx::query("UPDATE config_live_input SET options = 'invalid-json' WHERE backend = 'rtmp'")
            .execute(&pool)
            .await
            .unwrap();
        let result = complete_setup(
            &pool,
            controllers.clone(),
            queues.clone(),
            CancellationToken::new(),
            SystemStat::new(),
            request(&root),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(handles::count_users(&pool).await.unwrap(), 0);
        let global = handles::select_global(&pool).await.unwrap();
        let channel = handles::select_channel(&pool, &1).await.unwrap();

        assert!(!global.setup_completed);
        assert_eq!(global.logs, global_before.logs);
        assert_eq!(channel.storage, channel_before.storage);
        assert!(controllers.read().await.managers.is_empty());
        assert!(queues.lock().await.is_empty());
        sqlx::query("UPDATE config_live_input SET options = '{}' WHERE backend = 'rtmp'")
            .execute(&pool)
            .await
            .unwrap();
        complete_setup(
            &pool,
            controllers.clone(),
            queues.clone(),
            CancellationToken::new(),
            SystemStat::new(),
            request(&root),
        )
        .await
        .unwrap();

        assert!(handles::select_global(&pool).await.unwrap().setup_completed);
        assert_eq!(handles::count_users(&pool).await.unwrap(), 1);
        assert_eq!(controllers.read().await.managers.len(), 1);
        assert_eq!(queues.lock().await.len(), 1);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_setup_has_one_winner_and_does_not_create_loser_directories() {
        let (pool, controllers, queues) = setup_state().await;
        let root = root();
        let first_root = root.join("first");
        let second_root = root.join("second");
        let first = complete_setup(
            &pool,
            controllers.clone(),
            queues.clone(),
            CancellationToken::new(),
            SystemStat::new(),
            request(&first_root),
        );
        let second = complete_setup(
            &pool,
            controllers.clone(),
            queues.clone(),
            CancellationToken::new(),
            SystemStat::new(),
            request(&second_root),
        );
        let (first, second) = tokio::join!(first, second);

        assert_ne!(first.is_ok(), second.is_ok());
        let loser = if first.is_ok() {
            &second_root
        } else {
            &first_root
        };
        let error = if first.is_ok() {
            second.err().unwrap()
        } else {
            first.err().unwrap()
        };

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(!loser.exists());
        assert!(handles::select_global(&pool).await.unwrap().setup_completed);
        assert_eq!(handles::count_users(&pool).await.unwrap(), 1);
        assert_eq!(controllers.read().await.managers.len(), 1);
        assert_eq!(queues.lock().await.len(), 1);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    fn settings(storage: &str) -> SetupSettings {
        SetupSettings {
            logs: "/var/log/ffplayout".to_string(),
            playlists: "/var/lib/ffplayout/playlists".to_string(),
            public: "/usr/share/ffplayout/public".to_string(),
            storage: storage.to_string(),
            shared: false,
            smtp_server: String::new(),
            smtp_user: String::new(),
            smtp_password: String::new(),
            smtp_starttls: false,
            smtp_port: 465,
        }
    }

    #[test]
    fn setup_paths_allow_application_directories_and_custom_data_roots() {
        assert!(validate_setup_paths(&settings("/mnt/media")).is_ok());
        assert!(validate_setup_paths(&settings("/var/www/media")).is_ok());
    }

    #[test]
    fn setup_paths_reject_protected_system_directories() {
        assert!(validate_setup_paths(&settings("/etc/ffplayout")).is_err());
        assert!(validate_setup_paths(&settings("/var/lib")).is_err());
    }

    #[test]
    fn setup_paths_reject_relative_components() {
        assert!(validate_setup_paths(&settings("/mnt/../etc")).is_err());
        assert!(validate_setup_paths(&settings("media")).is_err());
        assert!(validate_setup_paths(&settings("/")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn setup_paths_allow_windows_drive_and_unc_paths() {
        assert!(validate_setup_path("Logging", r"C:\Users\jonathan\Videos\logs").is_ok());
        assert!(validate_setup_path("Logging", r"\\server\share\logs").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn setup_paths_reject_windows_roots_and_relative_components() {
        assert!(validate_setup_path("Logging", r"C:\").is_err());
        assert!(validate_setup_path("Logging", r"C:\logs\..\system").is_err());
    }
}
