use super::*;

/// Store a real password hash for a fixture user and return it.
pub(super) async fn set_password(state: &AppState, id: i32, password: &str) -> String {
    use argon2::{Argon2, PasswordHasher};

    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .unwrap()
        .to_string();

    sqlx::query("UPDATE auth_user SET password = $1 WHERE id = $2")
        .bind(&hash)
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();

    hash
}

fn import_request(uri: &str, token: &str) -> Request<Body> {
    let boundary = "ffplayout-test-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"list.m3u\"; filename=\"list.m3u\"\r\n\r\n# empty list\n\r\n--{boundary}--\r\n"
    );

    Request::builder()
        .method("PUT")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
#[serial]
async fn playlist_import_dates_cannot_escape_the_channel_directory() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let outside = fixture.root.join("outside");
    let escaping = format!("{}/a-b", outside.to_string_lossy());

    for date in [
        "",
        "invalid",
        "..%2F..%2Fx-y",
        escaping.as_str(),
        "2026-1-2",
    ] {
        let uri = format!("/api/file/1/import?file=list.m3u&date={date}");
        let response = fixture
            .app
            .clone()
            .oneshot(import_request(&uri, &user))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{date}");
    }

    assert!(!outside.exists());
    assert!(!fixture.root.join("x-y.json").exists());

    let response = fixture
        .app
        .clone()
        .oneshot(import_request(
            "/api/file/1/import?file=list.m3u&date=2026-11-16",
            &user,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        fixture
            .root
            .join("playlists/2026/11/2026-11-16.json")
            .is_file()
    );
}

#[tokio::test]
#[serial]
async fn log_dates_cannot_select_other_files() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;

    for date in [
        "x%2F..%2F..%2F..%2Fvar%2Flog%2Fauth",
        "x%2F..%2Fffplayout_2",
        "2026-11-16%2F..",
        "2026-1-2",
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/log/1?date={date}"),
                Some(&user),
                json!({}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{date}");
    }
}

#[tokio::test]
#[serial]
async fn storage_root_cannot_be_removed_or_renamed() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let storage = fixture.root.join("storage");

    tokio::fs::create_dir_all(&storage).await.unwrap();
    tokio::fs::write(storage.join("clip.mp4"), b"media")
        .await
        .unwrap();

    for source in ["", "/", "."] {
        let removed = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/file/1/remove",
                Some(&user),
                json!({"source": source, "recursive": true}),
            ))
            .await
            .unwrap();
        let renamed = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/file/1/rename",
                Some(&user),
                json!({"source": source, "target": "moved"}),
            ))
            .await
            .unwrap();

        assert_eq!(removed.status(), StatusCode::FORBIDDEN, "{source:?}");
        assert_eq!(renamed.status(), StatusCode::FORBIDDEN, "{source:?}");
    }

    assert!(storage.join("clip.mp4").is_file());
}

#[tokio::test]
#[serial]
async fn verification_code_is_dropped_after_repeated_wrong_attempts() {
    let fixture = security_fixture().await;
    let username = auth::store_verification(&fixture.state, 1, 0).await;

    for _ in 0..5 {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/auth/verify",
                None,
                json!({"username":username,"code":"000000"}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

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

    assert_ne!(response.status(), StatusCode::OK);
    assert!(
        !ffplayout::api::auth::VERIFICATION_CODES
            .lock()
            .await
            .contains_key(&username)
    );
}

#[tokio::test]
#[serial]
async fn health_probe_needs_no_authentication() {
    let fixture = security_fixture().await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request("GET", "/api/health", None, json!({})))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn own_password_and_mail_changes_need_the_current_password() {
    let fixture = security_fixture().await;
    let token = access_token(&fixture.state, 1).await;
    let hash = set_password(&fixture.state, 1, "current-password").await;

    for body in [
        json!({"username":"regular", "password":"new-password-1"}),
        json!({"username":"regular", "password":"new-password-1", "current_password":"wrong"}),
        json!({"username":"regular", "mail":"attacker@example.org"}),
        json!({"username":"regular", "mail":"attacker@example.org", "current_password":""}),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request("PUT", "/api/user/1", Some(&token), body))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    let unchanged = handles::select_login(&fixture.state.pool, "regular")
        .await
        .unwrap();

    assert_eq!(unchanged.password, hash);
    assert_eq!(unchanged.mail.as_deref(), Some("user@example.org"));

    // A short new password is rejected even with the right current one.
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PUT",
            "/api/user/1",
            Some(&token),
            json!({"username":"regular", "password":"short", "current_password":"current-password"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PUT",
            "/api/user/1",
            Some(&token),
            json!({"username":"regular", "password":"new-password-1", "current_password":"current-password"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let changed = handles::select_login(&fixture.state.pool, "regular")
        .await
        .unwrap();

    assert_ne!(changed.password, hash);

    // A global admin resets the password of another user without knowing it.
    let admin = access_token(&fixture.state, 2).await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PUT",
            "/api/user/1",
            Some(&admin),
            json!({"username":"regular", "password":"reset-password-1"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn removed_users_lose_their_access_token_immediately() {
    let fixture = security_fixture().await;
    let admin = access_token(&fixture.state, 2).await;
    let removed = access_token(&fixture.state, 1).await;
    let request = |token: &str| json_request("GET", "/api/user", Some(token), json!({}));
    let response = fixture
        .app
        .clone()
        .oneshot(request(&removed))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Token times have a resolution of one second.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "DELETE",
            "/api/user/1",
            Some(&admin),
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let response = fixture
        .app
        .clone()
        .oneshot(request(&removed))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Other accounts are not affected.
    let response = fixture.app.clone().oneshot(request(&admin)).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
