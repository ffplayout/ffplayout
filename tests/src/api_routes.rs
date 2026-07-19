use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::{get, post},
};
use serde_json::json;
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

use ffplayout::{
    api::{
        auth::{decode_jwt, decode_refresh_jwt, login, refresh},
        file_access::FileAccessState,
        state::AppState,
    },
    db::{
        handles, init_globales,
        models::{Role, User},
    },
    player::controller::{ChannelController, ChannelManager},
    sse::{SseAuthState, broadcast::Broadcaster},
    utils::{channels::delete_channel, config::PlayoutConfig, system::SystemStat},
};

async fn prepare_config() -> (PlayoutConfig, ChannelManager, Pool<Sqlite>) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET public = "assets/hls", logs = "assets/log", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE channels SET public = "assets/hls", playlists = "assets/playlists", storage = "assets/storage";
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

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
    .await;

    (config, manager, pool)
}

#[tokio::test]
async fn test_get() {
    let app = Router::new().route("/", get(StatusCode::OK));

    let res = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert!(res.status().is_success());
}

#[tokio::test]
async fn test_login() {
    let (_, manager, pool) = prepare_config().await;
    let app_state = AppState {
        auth_state: Arc::new(SseAuthState::default()),
        broadcaster: Broadcaster::create(manager.system.clone()),
        controller: Arc::new(RwLock::new(ChannelController::new())),
        file_access: Arc::new(FileAccessState::default()),
        mail_queues: Arc::new(Mutex::new(vec![])),
        pool: pool.clone(),
        shutdown: CancellationToken::new(),
        system: manager.system.clone(),
    };

    init_globales(&pool).await.unwrap();

    let app = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .with_state(app_state);

    let payload = json!({"username": "admin", "password": "admin"});

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(res.status().is_success());
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let tokens: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let access = tokens["access"].as_str().unwrap();
    let refresh_token = tokens["refresh"].as_str().unwrap();

    assert!(decode_jwt(access).await.is_ok());
    assert!(decode_refresh_jwt(refresh_token).await.is_ok());
    assert!(decode_jwt(refresh_token).await.is_err());
    assert!(decode_refresh_jwt(access).await.is_err());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({"refresh": access}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    sqlx::query("UPDATE user SET role_id = 3 WHERE username = 'admin'")
        .execute(&pool)
        .await
        .unwrap();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({"refresh": refresh_token}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success());
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let refreshed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let claims = decode_jwt(refreshed["access"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(claims.role, Role::User);

    let payload = json!({"username": "admin", "password": "1234"});

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 403);

    let payload = json!({"username": "aaa", "password": "1234"});

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn failed_start_restores_not_running_state() {
    let (_, manager, pool) = prepare_config().await;
    pool.close().await;

    assert!(manager.start().await.is_err());
    assert!(!manager.is_alive.load(Ordering::SeqCst));
    assert!(manager.supervisor_handle.lock().await.is_none());
}

#[tokio::test]
async fn deleting_channel_stops_and_removes_manager() {
    let (_, manager, pool) = prepare_config().await;
    manager.is_alive.store(true, Ordering::SeqCst);
    let controller = Arc::new(RwLock::new(ChannelController::new()));
    controller.write().await.add(manager.clone());
    let mail_queues = Arc::new(Mutex::new(Vec::new()));

    delete_channel(&pool, manager.id, controller.clone(), mail_queues)
        .await
        .unwrap();

    assert!(!manager.is_alive.load(Ordering::SeqCst));
    assert!(controller.read().await.get(manager.id).is_none());
    assert!(handles::select_channel(&pool, &manager.id).await.is_err());
}
