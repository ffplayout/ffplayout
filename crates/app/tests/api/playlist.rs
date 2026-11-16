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
    for date in ["2026-11-16", "2026-11-17"] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "DELETE",
                &format!("/api/playlist/1/{date}"),
                Some(&user),
                json!({}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(target.is_file());
    }
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn playlist_symlinks_outside_the_root_are_rejected_before_access() {
    use std::os::unix::fs::symlink;

    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let root = fixture.root.join("playlists");
    let outside = fixture.root.join("other-channel");
    let victim = outside.join("2026-11-16.json");

    tokio::fs::create_dir_all(&outside).await.unwrap();
    tokio::fs::write(&victim, b"unchanged").await.unwrap();
    tokio::fs::create_dir_all(root.join("2026")).await.unwrap();
    symlink(&outside, root.join("2026/11")).unwrap();
    tokio::fs::create_dir_all(root.join("2027/11"))
        .await
        .unwrap();
    symlink(&victim, root.join("2027/11/2027-11-16.json")).unwrap();
    symlink(&outside, root.join("2028")).unwrap();

    for date in ["2026-11-16", "2026-11-17", "2027-11-16", "2028-11-16"] {
        let manager = fixture.state.controller.read().await.get(1).unwrap();

        manager.config.write().await.general.generate = Some(vec![date.into()]);
        assert!(matches!(
            ffplayout::utils::generator::playlist_generator(&manager, None).await,
            Err(ffplayout::utils::errors::ServiceError::Forbidden(_))
        ));

        for (method, uri, body) in [
            (
                "POST",
                "/api/playlist/1".into(),
                json!({"date": date, "program": []}),
            ),
            ("GET", format!("/api/playlist/1?date={date}"), json!({})),
            ("DELETE", format!("/api/playlist/1/{date}"), json!({})),
            (
                "POST",
                format!("/api/playlist/1/generate/{date}"),
                json!({"template": {"sources": []}}),
            ),
        ] {
            let response = fixture
                .app
                .clone()
                .oneshot(json_request(method, &uri, Some(&user), body))
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
            assert_eq!(tokio::fs::read(&victim).await.unwrap(), b"unchanged");
            assert!(!outside.join("2026-11-17.json").exists());
            assert!(!outside.join("11").exists());
        }
    }
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn playlist_root_and_internal_symlinks_support_shared_days() {
    use std::os::unix::fs::symlink;

    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let manager = fixture.state.controller.read().await.get(1).unwrap();
    let root = fixture.root.join("playlists");
    let root_link = fixture.root.join("playlist-link");
    let source = root.join("archive/11/shared.json");
    let alias = root.join("2026/11/2026-11-16.json");

    tokio::fs::create_dir_all(root.join("archive/11"))
        .await
        .unwrap();
    tokio::fs::write(&source, br#"{"date":"2026-11-16","program":[]}"#)
        .await
        .unwrap();
    symlink("archive", root.join("2026")).unwrap();
    symlink("shared.json", &alias).unwrap();
    symlink("shared.json", root.join("2026/11/2026-11-18.json")).unwrap();
    symlink(&root, &root_link).unwrap();
    manager.config.write().await.channel.playlists = root_link;

    for (method, uri, body) in [
        ("GET", "/api/playlist/1?date=2026-11-16", json!({})),
        (
            "POST",
            "/api/playlist/1",
            json!({"channel": "Updated", "date":"2026-11-16", "program":[]}),
        ),
        ("DELETE", "/api/playlist/1/2026-11-16", json!({})),
        (
            "POST",
            "/api/playlist/1/generate/2026-11-17",
            json!({"template": {"sources": []}}),
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(method, uri, Some(&user), body))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "{method} {uri}");
    }

    let shared: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(&source).await.unwrap()).unwrap();

    assert_eq!(shared["channel"], "Updated");
    assert!(!alias.exists());
    assert!(root.join("2026/11/2026-11-18.json").is_file());
    assert!(root.join("archive/11/2026-11-17.json").is_file());
}

#[tokio::test]
#[serial]
async fn playlist_generation_rejects_invalid_dates_before_creating_paths() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;

    for date in ["invalid", "2026-02-30", "2026-11-%2F..%2F..%2Fescape-x"] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/playlist/1/generate/{date}"),
                Some(&user),
                json!({"template": {"sources": []}}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{date}");
        assert!(!fixture.root.join("playlists/2026").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn dangling_playlist_links_are_contained_and_can_be_removed_or_written() {
    use std::os::unix::fs::symlink;

    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let root = fixture.root.join("playlists");
    let month = root.join("2026/11");
    let source = month.join("shared.json");
    let outside = fixture.root.join("missing.json");

    tokio::fs::create_dir_all(&month).await.unwrap();
    symlink("shared.json", month.join("2026-11-16.json")).unwrap();
    symlink("shared.json", month.join("2026-11-17.json")).unwrap();
    symlink(&outside, month.join("2026-11-18.json")).unwrap();
    symlink("2026-11-20.json", month.join("2026-11-19.json")).unwrap();
    symlink("2026-11-19.json", month.join("2026-11-20.json")).unwrap();

    for (method, uri, body, expected) in [
        (
            "GET",
            "/api/playlist/1?date=2026-11-16",
            json!({}),
            StatusCode::NO_CONTENT,
        ),
        (
            "DELETE",
            "/api/playlist/1/2026-11-16",
            json!({}),
            StatusCode::OK,
        ),
        (
            "POST",
            "/api/playlist/1",
            json!({"date":"2026-11-17","program":[]}),
            StatusCode::OK,
        ),
        (
            "POST",
            "/api/playlist/1",
            json!({"date":"2026-11-18","program":[]}),
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/playlist/1/2026-11-18",
            json!({}),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(method, uri, Some(&user), body))
            .await
            .unwrap();

        assert_eq!(response.status(), expected, "{method} {uri}");
    }

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/playlist/1",
            Some(&user),
            json!({"date":"2026-11-19","program":[]}),
        ))
        .await
        .unwrap();

    assert!(!response.status().is_success());
    assert!(!month.join("2026-11-16.json").is_symlink());
    assert!(source.is_file());
    assert!(month.join("2026-11-17.json").is_symlink());
    assert!(month.join("2026-11-18.json").is_symlink());
    assert!(!outside.exists());
}
