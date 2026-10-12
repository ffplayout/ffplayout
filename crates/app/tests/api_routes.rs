use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use protect_axum::GrantsLayer;
use serde_json::json;
use serial_test::serial;
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

#[path = "api/auth.rs"]
mod auth;
#[path = "api/configuration.rs"]
mod configuration;
#[path = "api/files.rs"]
mod files;
#[path = "api/users.rs"]
mod users;

use ffplayout::{
    api::{
        auth::{decode_jwt, decode_refresh_jwt, login, logout, refresh},
        file_access::FileAccessState,
        path,
        state::AppState,
    },
    db::{
        handles, init_globales,
        models::{Role, TextPreset, User},
    },
    extract,
    player::controller::{ChannelController, ChannelManager},
    sse::{SseAuthState, broadcast::Broadcaster},
    utils::{config::PlayoutConfig, system::SystemStat},
};

async fn prepare_config() -> (PlayoutConfig, ChannelManager, Pool<Sqlite>) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets")
        .canonicalize()
        .unwrap();
    sqlx::query("UPDATE config_global SET public = $1, logs = $2, playlists = $3, storage = $4; UPDATE channels SET public = $1, playlists = $3, storage = $4;")
        .bind(assets.join("hls").to_string_lossy())
        .bind(assets.join("log").to_string_lossy())
        .bind(assets.join("playlists").to_string_lossy())
        .bind(assets.join("storage").to_string_lossy())
        .execute(&pool).await.unwrap();

    let user = User {
        id: 0,
        mail: Some("admin@mail.com".to_string()),
        username: "admin".to_string(),
        password: "admin".to_string(),
        role_id: Some(1),
        channel_ids: Some(vec![1]),
        token: None,
        two_factor: false,
    };

    handles::insert_user(&pool, user.clone()).await.unwrap();

    let config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(
        pool.clone(),
        channel,
        config.clone(),
        CancellationToken::new(),
        SystemStat::new(),
    )
    .await
    .expect("test storage should initialize");

    (config, manager, pool)
}

struct SecurityFixture {
    app: Router,
    state: AppState,
    root: std::path::PathBuf,
}

impl Drop for SecurityFixture {
    fn drop(&mut self) {
        self.state.shutdown.cancel();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn security_fixture() -> SecurityFixture {
    let root = std::env::temp_dir().join(format!("ffplayout-security-{}", uuid_for_test()));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();
    handles::create_channel_records(
        &pool,
        ffplayout::db::models::Channel {
            name: "other-channel".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE config_global SET logs = $1, playlists = $2, public = $3, storage = $4, shared = 0",
    )
    .bind(root.join("logs").to_string_lossy().as_ref())
    .bind(root.join("playlists").to_string_lossy().as_ref())
    .bind(root.join("public").to_string_lossy().as_ref())
    .bind(root.join("storage").to_string_lossy().as_ref())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE channels SET playlists = $1, public = $2, storage = $3")
        .bind(root.join("playlists").to_string_lossy().as_ref())
        .bind(root.join("public").to_string_lossy().as_ref())
        .bind(root.join("storage").to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id, two_factor) VALUES (1, 'user@example.org', 'regular', 'unchanged', 3, 1), (2, 'admin@example.org', 'administrator', 'unchanged', 1, 0)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO auth_user_channels (channel_id, user_id) VALUES (1, 1), (1, 2)")
        .execute(&pool)
        .await
        .unwrap();
    let _ = init_globales(&pool).await;
    let system = SystemStat::new();
    let shutdown = CancellationToken::new();
    let controller = Arc::new(RwLock::new(ChannelController::new()));

    for id in [1, 2] {
        let config = PlayoutConfig::new(&pool, id, None).await.unwrap();
        let channel = handles::select_channel(&pool, &id).await.unwrap();
        let manager = ChannelManager::new(
            pool.clone(),
            channel,
            config,
            shutdown.clone(),
            system.clone(),
        )
        .await
        .unwrap();
        controller.write().await.add(manager);
    }

    tokio::fs::create_dir_all(root.join("public/live"))
        .await
        .unwrap();
    let state = AppState {
        auth_state: Arc::new(SseAuthState::default()),
        broadcaster: Broadcaster::create(system.clone()),
        controller,
        file_access: Arc::new(FileAccessState::default()),
        mail_queues: Arc::new(Mutex::new(Vec::new())),
        pool,
        shutdown,
        system,
    };
    let app = path::routes()
        .with_state(state.clone())
        .layer(GrantsLayer::with_extractor(extract));

    SecurityFixture { app, state, root }
}

fn uuid_for_test() -> String {
    // Use a database-independent unique name without adding a test dependency.
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn access_token(state: &AppState, id: i32) -> String {
    let user = handles::select_user(&state.pool, id).await.unwrap();
    let role = handles::select_role(&state.pool, &user.role_id.unwrap())
        .await
        .unwrap();

    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::from_value(serde_json::json!({
        "id": user.id,
        "channels": user.channel_ids.unwrap_or_default(),
        "username": user.username,
        "role": role,
        "token_type": "access",
        "iat": now,
        "exp": now + 300,
    }))
    .unwrap();

    ffplayout::api::auth::encode_jwt(claims).await.unwrap()
}

fn json_request(
    method: &str,
    uri: &str,
    token: Option<&str>,
    data: serde_json::Value,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");

    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }

    request.body(Body::from(data.to_string())).unwrap()
}
