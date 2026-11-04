use super::*;

#[tokio::test]
#[serial]
async fn manual_graphics_authorization_revision_and_failure_isolation() {
    use std::sync::atomic::Ordering;

    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;

    for (channel, token, expected) in [
        (1, None, StatusCode::UNAUTHORIZED),
        (2, Some(user.as_str()), StatusCode::FORBIDDEN),
        (1, Some(user.as_str()), StatusCode::OK),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "GET",
                &format!("/api/graphics/{channel}"),
                token,
                json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }

    let manager = fixture.state.controller.read().await.get(1).unwrap();
    manager.is_alive.store(true, Ordering::SeqCst);
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets/storage/media_images/animated-logo.gif");
    tokio::fs::copy(source, fixture.root.join("storage/logo.gif"))
        .await
        .unwrap();

    for (body, expected) in [
        (
            json!({"revision": 0, "action": "take", "layer": 1, "source": "../secret.png"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"revision": 0, "action": "take", "layer": 1, "source": "logo.gif", "fade_in": 0, "fade_out": 0, "repeat": 1, "source_repeat": "forever", "source_end": "hide"}),
            StatusCode::OK,
        ),
        (
            json!({"revision": 0, "action": "clear_all"}),
            StatusCode::CONFLICT,
        ),
        (
            json!({"revision": 1, "action": "take", "layer": 2, "source": "missing.png"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"revision": 1, "action": "update", "layer": 1, "opacity": 2}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"revision": 1, "action": "update", "layer": 1, "position": [10, 20], "opacity": 0.5}),
            StatusCode::OK,
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request("POST", "/api/graphics/1", Some(&user), body))
            .await
            .unwrap();
        let code = response.status();
        let data = to_bytes(response.into_body(), 8192).await.unwrap();
        assert_eq!(code, expected, "{}", String::from_utf8_lossy(&data));
    }

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "GET",
            "/api/graphics/1",
            Some(&user),
            json!({}),
        ))
        .await
        .unwrap();
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(status["revision"], 2);
    assert_eq!(status["layers"].as_array().unwrap().len(), 1);
    assert_eq!(status["layers"][0]["position"], json!([10, 20]));
    assert_eq!(status["layers"][0]["running"], true);
    // Restart must reuse the admitted asset even if its file is no longer present.
    tokio::fs::remove_file(fixture.root.join("storage/logo.gif"))
        .await
        .unwrap();

    for (revision, action, running) in [(2, "stop", false), (3, "start", true)] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/graphics/1",
                Some(&user),
                json!({"revision": revision, "action": action, "layer": 1}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "GET",
                "/api/graphics/1",
                Some(&user),
                json!({}),
            ))
            .await
            .unwrap();
        let status: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(status["revision"], revision + 1);
        assert_eq!(status["layers"][0]["running"], running);
        assert_eq!(status["layers"][0]["position"], json!([10, 20]));
        assert_eq!(status["layers"][0]["opacity"], 0.5);
    }

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/graphics/1",
            Some(&user),
            json!({"revision": 4, "action": "clear_all"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    manager.is_alive.store(false, Ordering::SeqCst);
    for (channel, expected) in [(2, StatusCode::FORBIDDEN), (1, StatusCode::OK)] {
        let response = fixture.app.clone().oneshot(json_request(
            "POST", &format!("/api/graphics/{channel}"), Some(&user),
            json!({"revision":5,"action":"save","layer":0,"name":"Stored while stopped", "settings":{"source":"not-present.png"}}),
        )).await.unwrap();
        assert_eq!(response.status(), expected);
    }

    let (revision, saved) = handles::select_graphic_layers(&fixture.state.pool, 1)
        .await
        .unwrap();
    assert_eq!(revision, 6);
    assert_eq!(saved[0].name, "Stored while stopped");
    assert!(!saved[0].running);
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/graphics/1",
            Some(&user),
            json!({"revision":6,"action":"clear","layer":0}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        handles::select_graphic_layers(&fixture.state.pool, 1)
            .await
            .unwrap()
            .1
            .is_empty()
    );
}

#[tokio::test]
#[serial]
async fn logo_tab_updates_only_logo_configuration_and_requires_admin() {
    let fixture = security_fixture().await;
    let user = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, 2).await;
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "GET",
            "/api/playout/config/1",
            Some(&admin),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut config: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    let manager = fixture.state.controller.read().await.get(1).unwrap();
    manager
        .is_alive
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets/storage/media_images/animated-logo.gif");
    tokio::fs::copy(source, fixture.root.join("storage/logo.gif"))
        .await
        .unwrap();
    config["processing"]["add_logo"] = json!(true);
    config["processing"]["logo"] = json!("logo.gif");
    config["processing"]["logo_position"] = json!("0:0");
    let original_mode = config["processing"]["mode"].clone();
    config["processing"]["mode"] = json!("folder");
    config["processing"]["logo_opacity"] = json!(0.37);

    for (token, expected) in [(&user, StatusCode::FORBIDDEN), (&admin, StatusCode::OK)] {
        let response = fixture
            .app
            .clone()
            .oneshot(json_request(
                "PUT",
                "/api/playout/config/1?scope=logo",
                Some(token),
                config.clone(),
            ))
            .await
            .unwrap();
        let code = response.status();
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        assert_eq!(code, expected, "{}", String::from_utf8_lossy(&body));

        if code == StatusCode::OK {
            let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(status["requires_restart"], false);
            assert!(manager.is_alive.load(std::sync::atomic::Ordering::SeqCst));
        }
    }

    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "GET",
            "/api/playout/config/1",
            Some(&admin),
            json!({}),
        ))
        .await
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(saved["processing"]["logo_opacity"], 0.37);
    assert_eq!(saved["processing"]["mode"], original_mode);
    let mut invalid = config.clone();
    invalid["processing"]["logo_position"] = json!("invalid-layout");
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PUT",
            "/api/playout/config/1?scope=logo",
            Some(&admin),
            invalid,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(
            "PUT",
            "/api/playout/config/1?scope=logo",
            Some(&admin),
            config,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(status["requires_restart"], false);
    manager.stop_all(false).await;
}
