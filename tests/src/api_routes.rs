use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::{get, post},
};
use protect_axum::GrantsLayer;
use serde_json::json;
use serial_test::serial;
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

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
        UPDATE config_global SET public = "assets/hls", logs = "assets/log", playlists = "assets/playlists", storage = "assets/storage";
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
    .await
    .expect("test storage should initialize");

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

    let _ = init_globales(&pool).await;

    let app = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
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

    sqlx::query("UPDATE auth_user SET role_id = 3 WHERE username = 'admin'")
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
    let rotated_refresh = refreshed["refresh"].as_str().unwrap();
    assert!(decode_refresh_jwt(rotated_refresh).await.is_ok());

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
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({"refresh": rotated_refresh}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username": "admin", "password": "admin"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(res.status().is_success());
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let tokens: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let logout_refresh = tokens["refresh"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("content-type", "application/json")
                .body(Body::from(json!({"refresh": logout_refresh}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({"refresh": logout_refresh}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

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
#[serial]
async fn configuration_output_and_preset_routes_cover_crud_and_permissions() {
    let (_, manager, pool) = prepare_config().await;
    let controller = Arc::new(RwLock::new(ChannelController::new()));
    controller.write().await.add(manager.clone());
    let app_state = AppState {
        auth_state: Arc::new(SseAuthState::default()),
        broadcaster: Broadcaster::create(manager.system.clone()),
        controller,
        file_access: Arc::new(FileAccessState::default()),
        mail_queues: Arc::new(Mutex::new(vec![])),
        pool: pool.clone(),
        shutdown: CancellationToken::new(),
        system: manager.system.clone(),
    };
    let _ = init_globales(&pool).await;
    let login_app = Router::new()
        .route("/auth/login", post(login))
        .with_state(app_state.clone());
    let login_response = login_app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username": "admin", "password": "admin"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);
    let login_body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_json: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let admin_token = login_json["access"].as_str().unwrap().to_string();
    let app = path::routes()
        .with_state(app_state.clone())
        .layer(GrantsLayer::with_extractor(extract));

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/playout/config/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let config_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/playout/config/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(config_response.status(), StatusCode::OK);
    let config_body = to_bytes(config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let mut config: PlayoutConfig = serde_json::from_slice(&config_body).unwrap();

    let outputs_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/playout/outputs/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(outputs_response.status(), StatusCode::OK);
    let outputs_body = to_bytes(outputs_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let outputs: Vec<ffplayout::db::models::Output> =
        serde_json::from_slice(&outputs_body).unwrap();
    assert_eq!(outputs.len(), 3);
    assert!(outputs.iter().all(|output| output.channel_id == 1));

    let global_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/global")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_response.status(), StatusCode::OK);
    let global_body = to_bytes(global_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let global_json: serde_json::Value = serde_json::from_slice(&global_body).unwrap();
    assert!(global_json.get("secret").is_none());
    assert!(global_json.get("smtp_password").is_none());
    assert!(global_json.get("notification_token").is_none());

    let update_global_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/global")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "smtp_server": "smtp.example.org",
                        "smtp_user": "alerts@example.org",
                        "smtp_password": "secret",
                        "smtp_starttls": true,
                        "smtp_port": 587,
                        "notification_server": "https://push.example.org/",
                        "notification_token": "push-secret"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_global_response.status(), StatusCode::OK);
    let global = handles::select_global(&pool).await.unwrap();
    assert_eq!(global.smtp_server, "smtp.example.org");
    assert_eq!(global.smtp_user, "alerts@example.org");
    assert_eq!(global.smtp_password, "secret");
    assert!(global.smtp_starttls);
    assert_eq!(global.smtp_port, 587);
    assert_eq!(global.notification_server, "https://push.example.org");
    assert_eq!(global.notification_token, "push-secret");

    let refreshed_config_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/playout/config/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refreshed_config_response.status(), StatusCode::OK);
    let refreshed_config_body = to_bytes(refreshed_config_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let refreshed_config: serde_json::Value =
        serde_json::from_slice(&refreshed_config_body).unwrap();
    assert_eq!(refreshed_config["notification"]["show"], true);

    let runtime_config = manager.config.read().await;
    assert_eq!(
        runtime_config.notification.server,
        "https://push.example.org"
    );
    assert_eq!(runtime_config.notification.token, "push-secret");
    drop(runtime_config);

    let preset = TextPreset {
        name: "HTTP preset".to_string(),
        text: "Initial text".to_string(),
        use_filename: true,
        ..TextPreset::default()
    };
    let add_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/presets/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&preset).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_response.status(), StatusCode::OK);
    let preset_id: i32 =
        sqlx::query_scalar("SELECT id FROM config_text_presets WHERE name = 'HTTP preset'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let presets_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/presets/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(presets_response.status(), StatusCode::OK);
    let presets_body = to_bytes(presets_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let presets: Vec<serde_json::Value> = serde_json::from_slice(&presets_body).unwrap();
    assert!(
        presets
            .iter()
            .any(|preset| preset["id"].as_i64() == Some(i64::from(preset_id)))
    );

    let mut updated_preset = preset;
    updated_preset.id = preset_id;
    updated_preset.text = "Updated text".to_string();
    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/presets/1/{preset_id}"))
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&updated_preset).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    assert_eq!(
        handles::select_preset(&pool, 1, preset_id)
            .await
            .unwrap()
            .text,
        "Updated text"
    );
    updated_preset.persistent = true;
    let persistent_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/control/1/text")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&updated_preset).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(persistent_response.status(), StatusCode::OK);
    assert!(
        handles::select_preset(&pool, 1, preset_id)
            .await
            .unwrap()
            .persistent
    );

    updated_preset.persistent = false;
    let transient_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/control/1/text")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&updated_preset).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(transient_response.status(), StatusCode::OK);
    assert_eq!(
        handles::select_configuration(&pool, 1)
            .await
            .unwrap()
            .text_preset_id,
        None
    );

    config.mail.subject = "Updated through API".to_string();
    config.notification.topic = "ffplayout-alerts".to_string();
    config.notification.tags = "warning,broadcast".to_string();
    config.text.preset_id = Some(preset_id);
    let update_config_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/playout/config/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&config).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_config_response.status(), StatusCode::OK);
    let stored = handles::select_configuration(&pool, 1).await.unwrap();
    assert_eq!(stored.mail_subject, "Updated through API");
    assert_eq!(stored.notification_topic, "ffplayout-alerts");
    assert_eq!(stored.notification_tags, "warning,broadcast");
    assert_eq!(stored.text_preset_id, Some(preset_id));

    let mut invalid = config.clone();
    invalid.mail.subject = "Must roll back".to_string();
    invalid.output.id = i32::MAX;
    let invalid_output_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/playout/config/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&invalid).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_output_response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        handles::select_configuration(&pool, 1)
            .await
            .unwrap()
            .mail_subject,
        "Updated through API"
    );

    let mut invalid_preset = config.clone();
    invalid_preset.text.preset_id = Some(i32::MAX);
    let invalid_preset_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/playout/config/1")
                .header("authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&invalid_preset).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_preset_response.status(), StatusCode::BAD_REQUEST);

    let missing_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/presets/999/{preset_id}"))
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

    sqlx::query("UPDATE auth_user SET role_id = 3 WHERE username = 'admin'")
        .execute(&pool)
        .await
        .unwrap();
    let user_login_response = Router::new()
        .route("/auth/login", post(login))
        .with_state(app_state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username": "admin", "password": "admin"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let user_login_body = to_bytes(user_login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let user_login_json: serde_json::Value = serde_json::from_slice(&user_login_body).unwrap();
    let user_token = user_login_json["access"].as_str().unwrap();
    let forbidden_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/presets/999")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);

    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/presets/1/{preset_id}"))
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    assert_eq!(
        handles::select_configuration(&pool, 1)
            .await
            .unwrap()
            .text_preset_id,
        None
    );
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
