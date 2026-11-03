use super::*;

#[tokio::test]
#[serial]
async fn selection_browser_enforces_channel_access_and_lists_file_details() {
    let fixture = security_fixture().await;
    tokio::fs::write(fixture.root.join("storage/captions.vtt"), b"plain text")
        .await
        .unwrap();
    tokio::fs::write(fixture.root.join("storage/task.sh"), b"plain text")
        .await
        .unwrap();
    let user = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, 2).await;

    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(user.as_str()), StatusCode::OK),
        (Some(admin.as_str()), StatusCode::OK),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/file/1/browse-selection",
                token,
                json!({"source":""}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);

        if expected == StatusCode::OK {
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                    .unwrap();
            assert_eq!(value["files"].as_array().unwrap().len(), 2);
            assert_eq!(value["files"][0]["name"], "captions.vtt");
            assert_eq!(value["files"][0]["size"], 10);
            assert_eq!(value["files"][1]["name"], "task.sh");
            assert_eq!(value["files"][1]["size"], 10);
        }
    }
    // One second of mono PCM; duration probing must run only for the selected file.
    let samples = 48_000_u32;
    let data_size = samples * 2;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&samples.to_le_bytes());
    wav.extend_from_slice(&(samples * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.resize(44 + data_size as usize, 0);
    tokio::fs::write(fixture.root.join("storage/clip.wav"), &wav)
        .await
        .unwrap();
    // A subdirectory with the root's basename must not be stripped by probing.
    tokio::fs::create_dir_all(fixture.root.join("storage/storage"))
        .await
        .unwrap();
    let longer_size = data_size * 2;
    wav[4..8].copy_from_slice(&(36 + longer_size).to_le_bytes());
    wav[40..44].copy_from_slice(&longer_size.to_le_bytes());
    wav.resize(44 + longer_size as usize, 0);
    tokio::fs::write(fixture.root.join("storage/storage/clip.wav"), wav)
        .await
        .unwrap();
    for (channel, source, token, expected) in [
        (1, "clip.wav", None, StatusCode::UNAUTHORIZED),
        (1, "clip.wav", Some(user.as_str()), StatusCode::OK),
        (1, "storage/clip.wav", Some(user.as_str()), StatusCode::OK),
        (2, "clip.wav", Some(user.as_str()), StatusCode::FORBIDDEN),
        (
            1,
            "../secret.wav",
            Some(user.as_str()),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/file/{channel}/media-info"),
                token,
                json!({"source":source}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert_eq!(
                value["duration"],
                if source == "storage/clip.wav" {
                    2.0
                } else {
                    1.0
                }
            );
        }
    }

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/file/2/browse-selection",
            Some(&user),
            json!({"source":""}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    sqlx::query("UPDATE auth_user SET role_id = 4 WHERE id = 1")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let guest = access_token(&fixture.state, 1).await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/file/1/browse-selection",
            Some(&guest),
            json!({"source":""}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    sqlx::query("UPDATE auth_user SET role_id = 2 WHERE id = 1")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    let channel_admin = access_token(&fixture.state, 1).await;

    for (channel, source, expected) in [
        (1, String::new(), StatusCode::OK),
        (2, String::new(), StatusCode::FORBIDDEN),
        (
            1,
            fixture.root.join("outside").to_string_lossy().into_owned(),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/file/{channel}/browse-selection"),
                Some(&channel_admin),
                json!({"source": source}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
#[serial]
async fn file_routes_enforce_tokens_channels_and_exact_range_responses() {
    let fixture = security_fixture().await;
    tokio::fs::write(fixture.root.join("storage/video.mp4"), b"0123456789")
        .await
        .unwrap();
    tokio::fs::write(fixture.root.join("storage/other.mp4"), b"other")
        .await
        .unwrap();
    let token = access_token(&fixture.state, 1).await;
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/file/1/video.mp4")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/file/2/video.mp4")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/file/1/video.mp4")
                .header("authorization", format!("Bearer {token}"))
                .header("range", "bytes=2-4")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-range"], "bytes 2-4/10");
    assert_eq!(response.headers()["content-length"], "3");
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
        b"234"
    );
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/file/1/video.mp4")
                .header("authorization", format!("Bearer {token}"))
                .header("range", "bytes=99-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/file/2/access-token",
            Some(&token),
            json!({"filename":"video.mp4"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/file/1/access-token",
            Some(&token),
            json!({"filename":"video.mp4"}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap();
    let access = value["access"].as_str().unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/file/1/video.mp4?access={access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
        b"0123456789"
    );

    for uri in [
        format!("/file/1/other.mp4?access={access}"),
        format!("/file/2/video.mp4?access={access}"),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
#[serial]
async fn file_and_public_routes_reject_encoded_traversal_and_external_symlinks() {
    let fixture = security_fixture().await;
    let token = access_token(&fixture.state, 1).await;
    tokio::fs::write(fixture.root.join("public/live/stream.m3u8"), b"playlist")
        .await
        .unwrap();
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/public/1/live/stream.m3u8")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
        b"playlist"
    );

    for uri in [
        "/file/1/%2e%2e/secret.mp4",
        "/file/1/sub/%2e%2e/%2e%2e/secret.mp4",
        "/public/1/live/%2e%2e/stream.m3u8",
        "/public/1/%2e%2e/secret.m3u8",
        "/public/1/%2Ftmp/secret.m3u8",
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
    }

    #[cfg(unix)]
    {
        let outside = fixture.root.join("outside");
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("secret.mp4"), b"secret")
            .await
            .unwrap();
        tokio::fs::write(outside.join("secret.m3u8"), b"secret")
            .await
            .unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("storage/escape")).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("public/escape")).unwrap();

        for uri in ["/file/1/escape/secret.mp4", "/public/1/escape/secret.m3u8"] {
            let response = fixture
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
    }
}
