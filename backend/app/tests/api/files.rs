use super::*;

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
