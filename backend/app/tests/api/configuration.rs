use super::*;

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

    // Invalid transport options must fail before any topic is persisted.
    for (key, value) in [
        ("rtmp_app", "abc\0def"),
        ("tcp_nodelay", "true"),
        ("rtmp_buffer ", "3000"),
    ] {
        let mut invalid = config.clone();
        invalid.mail.subject = "Must not be saved".to_string();
        invalid.output.mode = ffplayout::utils::config::OutputMode::Stream;
        invalid.output.stream_type = ffplayout::utils::config::StreamType::Rtmp;
        invalid.output.stream_url = "rtmp://127.0.0.1/live/test".to_string();
        invalid.output.protocol_options =
            [(key.to_string(), value.to_string())].into_iter().collect();
        let response = app
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("option"), "{body}");
    }
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
