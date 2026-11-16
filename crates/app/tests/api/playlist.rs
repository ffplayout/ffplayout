use super::*;

#[tokio::test]
#[serial]
async fn playlist_dates_cannot_escape_the_channel_directory() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let victim = fixture.root.join("victim-x.json");
    let new_target = fixture.root.join("new-directory/playlist-x");

    tokio::fs::write(&victim, b"unchanged").await.unwrap();

    let dates = [
        fixture.root.join("victim-x").to_string_lossy().into_owned(),
        new_target.to_string_lossy().into_owned(),
        "2026-11-/../../../victim-x".into(),
        "2026-11-16/../../victim-x".into(),
        r"C:\temp\playlist-x".into(),
        "2026-02-30".into(),
        "2026-1-2".into(),
        "2026-11-16.json".into(),
        "invalid".into(),
        String::new(),
    ];

    for date in dates {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/playlist/1",
                Some(&user),
                json!({"date": date, "program": []}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{date}");

        // Encoded separators reach the date extractor as part of one value.
        let encoded = date
            .replace('%', "%25")
            .replace('/', "%2F")
            .replace('\\', "%5C");

        for (method, uri) in [
            ("GET", format!("/api/playlist/1?date={encoded}")),
            ("DELETE", format!("/api/playlist/1/{encoded}")),
        ] {
            if method == "DELETE" && date.is_empty() {
                continue;
            }

            let response = fixture
                .app
                .clone()
                .oneshot(json_request(method, &uri, Some(&user), json!({})))
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{method}: {date}"
            );
        }

        assert_eq!(tokio::fs::read(&victim).await.unwrap(), b"unchanged");
        assert!(!fixture.root.join("new-directory").exists());
        assert!(!fixture.root.join("playlists/2026").exists());
    }
}

#[tokio::test]
#[serial]
async fn playlist_crud_preserves_channel_authorization_and_valid_dates() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let body = json!({"channel": "Test", "date": "2024-02-29", "program": []});

    for (channel, token, expected) in [
        (1, None, StatusCode::UNAUTHORIZED),
        (2, Some(user.as_str()), StatusCode::FORBIDDEN),
        (1, Some(user.as_str()), StatusCode::OK),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/playlist/{channel}"),
                token,
                body.clone(),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), expected);
    }

    let target = fixture.root.join("playlists/2024/02/2024-02-29.json");

    assert!(target.is_file());

    for (method, uri, body) in [
        (
            "POST",
            "/api/playlist/1",
            json!({"channel": "Updated", "date": "2024-02-29", "program": []}),
        ),
        ("GET", "/api/playlist/1?date=2024-02-29", json!({})),
        ("DELETE", "/api/playlist/1/2024-02-29", json!({})),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(method, uri, Some(&user), body))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        if method == "GET" {
            let data = to_bytes(response.into_body(), 8192).await.unwrap();
            let loaded: serde_json::Value = serde_json::from_slice(&data).unwrap();

            assert_eq!(loaded["channel"], "Updated");
            assert_eq!(loaded["date"], "2024-02-29");
        }
    }

    assert!(!target.exists());
}

#[tokio::test]
#[serial]
async fn playlist_fixed_json_target_still_validates_the_date() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let manager = fixture.state.controller.read().await.get(1).unwrap();
    let target = fixture.root.join("fixed.JSON");

    manager.config.write().await.channel.playlists = target.clone();

    for (date, expected) in [
        ("invalid", StatusCode::BAD_REQUEST),
        ("2026-11-16", StatusCode::OK),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/playlist/1",
                Some(&user),
                json!({"date": date, "program": []}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), expected);
        assert_eq!(target.exists(), expected == StatusCode::OK);
    }
}
