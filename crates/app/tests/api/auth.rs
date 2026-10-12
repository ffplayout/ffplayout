use super::*;

#[tokio::test]
#[serial]
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
async fn verification_rejects_wrong_expired_and_replayed_codes() {
    let fixture = security_fixture().await;
    let username = store_verification(&fixture.state, 1, 0).await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"wrong"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        ffplayout::api::auth::VERIFICATION_CODES
            .lock()
            .await
            .contains_key(&username)
    );
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"123456"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert!(decode_jwt(value["access"].as_str().unwrap()).await.is_ok());
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"123456"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    store_verification(&fixture.state, 1, 301).await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"123456"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !ffplayout::api::auth::VERIFICATION_CODES
            .lock()
            .await
            .contains_key(&username)
    );
}

#[tokio::test]
#[serial]
async fn verification_uses_current_rights_and_rejects_deleted_users() {
    let fixture = security_fixture().await;
    let username = store_verification(&fixture.state, 2, 0).await;
    sqlx::query("UPDATE auth_user SET role_id = 3 WHERE id = 2")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    handles::delete_user_channels(&fixture.state.pool, 2)
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"123456"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    let claims = decode_jwt(value["access"].as_str().unwrap()).await.unwrap();

    assert_eq!(claims.role, Role::User);
    assert!(!claims.channels.contains(&1));
    let username = store_verification(&fixture.state, 1, 0).await;
    handles::delete_user(&fixture.state.pool, 1).await.unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/auth/verify",
            None,
            json!({"username":username,"code":"123456"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !ffplayout::api::auth::VERIFICATION_CODES
            .lock()
            .await
            .contains_key(&username)
    );
}

async fn store_verification(state: &AppState, id: i32, age_seconds: i64) -> String {
    let user = handles::select_user(&state.pool, id).await.unwrap();
    let username = user.username.clone();
    let role = handles::select_role(&state.pool, &user.role_id.unwrap())
        .await
        .unwrap();
    ffplayout::api::auth::VERIFICATION_CODES
        .lock()
        .await
        .insert(
            username.clone(),
            ffplayout::api::auth::VerificationCode {
                code: "123456".to_string(),
                user,
                role,
                created_at: chrono::Utc::now() - chrono::TimeDelta::seconds(age_seconds),
            },
        );

    username
}
