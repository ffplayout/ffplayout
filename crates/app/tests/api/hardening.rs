use super::*;

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
