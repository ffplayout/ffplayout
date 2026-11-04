use std::{path::Path, sync::atomic::Ordering, time::Duration};

use serde_json::json;
use tokio::time::{Instant, sleep};
use tokio_util::sync::CancellationToken;

use crate::{db::handles, test_support, utils::system::SystemStat};

use super::*;

async fn saved(manager: &ChannelManager, revision: u64, id: u64, source: &str) -> u64 {
    command(manager.clone(), revision, json!({
        "action":"save", "layer":id, "name":format!("Layer {id}"),
        "settings":{"action":"take","at":0,"layer":id,"source":source,"fade_in":0.1,"fade_out":0.2,"source_repeat":"forever"}
    })).await.unwrap()
}

async fn wait_for_restore(manager: &ChannelManager) {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let status = status(manager).await.unwrap();

        if manager.manual_graphics.status().1.len() == 1 && status["layers"][1]["error"].is_string()
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "restore did not finish: {status}"
        );
        sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn saved_settings_and_requested_states_survive_fresh_manager_and_restore_failures_are_isolated()
 {
    let (manager, pool) = test_support::channel_manager().await;
    let root = manager.config.read().await.storage.path.clone();
    tokio::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_images/animated-logo.gif"),
        root.join("logo.gif"),
    )
    .await
    .unwrap();
    let mut revision = saved(&manager, 0, 1, "logo.gif").await;
    assert!(manager.manual_graphics.status().1.is_empty());
    assert_eq!(
        status(&manager).await.unwrap()["layers"][0]["running"],
        false
    );

    for (id, source, start, hide) in [
        (2, "missing.gif", true, false),
        (3, "logo.gif", true, true),
        (4, "logo.gif", false, false),
    ] {
        revision = saved(&manager, revision, id, source).await;

        if start {
            revision = command(
                manager.clone(),
                revision,
                json!({"action":"start","layer":id}),
            )
            .await
            .unwrap();
        }

        if hide {
            revision = command(
                manager.clone(),
                revision,
                json!({"action":"visibility","layer":id,"visible":false}),
            )
            .await
            .unwrap();
        }
    }

    revision = command(
        manager.clone(),
        revision,
        json!({"action":"start","layer":1}),
    )
    .await
    .unwrap();
    assert!(
        manager.manual_graphics.status().1.is_empty(),
        "stopped channels must not prepare graphics"
    );
    let config = manager.config.read().await.clone();
    let fresh = ChannelManager::new(
        pool.clone(),
        handles::select_channel(&pool, &1).await.unwrap(),
        config,
        CancellationToken::new(),
        SystemStat::new(),
    )
    .await
    .unwrap();
    fresh.is_alive.store(true, Ordering::SeqCst);
    fresh.graphics.resume();
    begin_restore(&fresh).await;
    wait_for_restore(&fresh).await;
    assert_eq!(fresh.manual_graphics.status().1[0].id, 1);
    let before = handles::select_graphic_layers(&pool, 1).await.unwrap();
    assert_eq!(before.0 as u64, revision);
    assert_eq!(before.1.len(), 4);
    assert!(!before.1[2].visible);
    assert!(!before.1[3].running);
    let revision = command(
        fresh.clone(),
        revision,
        json!({"action":"visibility","layer":3,"visible":true}),
    )
    .await
    .unwrap();
    assert_eq!(fresh.manual_graphics.status().1.len(), 2);
    assert!(
        command(fresh.clone(), revision, json!({"action":"start","layer":2}))
            .await
            .is_err()
    );
    assert_eq!(
        handles::select_graphic_layers(&pool, 1).await.unwrap().0 as u64,
        revision
    );
    fresh.is_alive.store(false, Ordering::SeqCst);
    fresh.graphics.cancel();
    stop_runtime(&fresh).await;
    fresh.graphics.join(1).await;
    assert!(fresh.manual_graphics.status().1.is_empty());
    assert_eq!(
        handles::select_graphic_layers(&pool, 1)
            .await
            .unwrap()
            .1
            .len(),
        4
    );
    fresh.is_alive.store(true, Ordering::SeqCst);
    fresh.graphics.resume();
    begin_restore(&fresh).await;
    // Cancel a restoration in flight; it must not publish into the next run.
    fresh.is_alive.store(false, Ordering::SeqCst);
    fresh.graphics.cancel();
    stop_runtime(&fresh).await;
    fresh.graphics.join(1).await;
    assert!(fresh.manual_graphics.status().1.is_empty());
    assert_eq!(fresh.graphics.preparation().budget().usage(), (0, 0, 0));
    pool.close().await;
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn bad_settings_stale_revisions_and_layer_limit_never_overwrite_saved_definitions() {
    let (manager, pool) = test_support::channel_manager().await;
    let root = manager.config.read().await.storage.path.clone();
    let revision = saved(&manager, 0, 1, "logo.png").await;
    assert!(
        command(manager.clone(), 0, json!({"action":"clear_all"}))
            .await
            .is_err()
    );

    for settings in [
        json!({"source":"../secret.png"}),
        json!({"source":"logo.png","opacity":2}),
        json!({"source":"logo.png","size":[3,4]}),
        json!({"source":"logo.png","fade_in":-1}),
    ] {
        assert!(
            command(
                manager.clone(),
                revision,
                json!({"action":"save","layer":1,"settings":settings})
            )
            .await
            .is_err()
        );
    }

    assert_eq!(handles::select_graphic_layers(&pool, 1).await.unwrap().0, 1);
    let mut revision = revision;

    for id in 2..=8 {
        revision = saved(&manager, revision, id, "logo.png").await;
    }

    assert!(
        command(
            manager.clone(),
            revision,
            json!({"action":"save","layer":9,"settings":{"source":"logo.png"}})
        )
        .await
        .is_err()
    );
    assert_eq!(
        status(&manager).await.unwrap()["layers"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    command(manager.clone(), revision, json!({"action":"clear_all"}))
        .await
        .unwrap();
    assert!(
        status(&manager).await.unwrap()["layers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    pool.close().await;
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn hiding_and_stopping_keep_working_when_the_prepared_sources_symlink_becomes_invalid() {
    let (manager, pool) = test_support::channel_manager().await;
    let root = manager.config.read().await.storage.path.clone();
    let asset = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets/storage/media_images/animated-logo.gif")
        .canonicalize()
        .unwrap();
    tokio::fs::copy(&asset, root.join("real.gif"))
        .await
        .unwrap();
    let link = root.join("logo.gif");
    std::os::unix::fs::symlink(root.join("real.gif"), &link).unwrap();
    manager.is_alive.store(true, Ordering::SeqCst);
    manager.graphics.resume();
    let revision = command(
        manager.clone(),
        0,
        json!({"action":"take","layer":1,"source":"logo.gif"}),
    )
    .await
    .unwrap();
    assert!(manager.manual_graphics.status().1[0].running);
    tokio::fs::remove_file(&link).await.unwrap();
    std::os::unix::fs::symlink(&asset, &link).unwrap();
    let revision = command(
        manager.clone(),
        revision,
        json!({"action":"visibility","layer":1,"visible":false}),
    )
    .await
    .unwrap();
    assert!(!manager.manual_graphics.status().1[0].visible);
    let revision = command(
        manager.clone(),
        revision,
        json!({"action":"stop","layer":1}),
    )
    .await
    .unwrap();
    assert!(!manager.manual_graphics.status().1[0].running);
    let (stored_revision, rows) = handles::select_graphic_layers(&pool, 1).await.unwrap();
    assert_eq!(stored_revision as u64, revision);
    assert!(!rows[0].running && !rows[0].visible);
    assert!(
        command(
            manager.clone(),
            revision,
            json!({"action":"take","layer":1,"source":"logo.gif"})
        )
        .await
        .is_err()
    );
    assert_eq!(
        handles::select_graphic_layers(&pool, 1).await.unwrap().0 as u64,
        revision
    );
    manager.is_alive.store(false, Ordering::SeqCst);
    manager.graphics.cancel();
    stop_runtime(&manager).await;
    manager.graphics.join(1).await;
    pool.close().await;
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn applying_and_saving_fades_update_existing_controls_without_preparing_the_source_again() {
    let (manager, pool) = test_support::channel_manager().await;
    let root = manager.config.read().await.storage.path.clone();
    tokio::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_images/animated-logo.gif"),
        root.join("logo.gif"),
    )
    .await
    .unwrap();
    manager.is_alive.store(true, Ordering::SeqCst);
    manager.graphics.resume();
    let revision = command(
        manager.clone(),
        0,
        json!({"action":"take","layer":1,"source":"logo.gif","fade_in":0,"fade_out":0}),
    )
    .await
    .unwrap();
    tokio::fs::remove_file(root.join("logo.gif")).await.unwrap();
    let usage = manager.graphics.preparation().budget().usage();
    let revision = command(
        manager.clone(),
        revision,
        json!({"action":"update","layer":1,"fade_in":0.2,"fade_out":0.4}),
    )
    .await
    .unwrap();
    let (_, rows) = handles::select_graphic_layers(&pool, 1).await.unwrap();
    assert_eq!(rows[0].settings["fade_in"], 0.2);
    assert_eq!(rows[0].settings["fade_out"], 0.4);
    let revision = command(
        manager.clone(),
        revision,
        json!({"action":"visibility","layer":1,"visible":false}),
    )
    .await
    .unwrap();
    assert!(!manager.manual_graphics.status().1[0].visible);
    let revision = command(
        manager.clone(),
        revision,
        json!({"action":"visibility","layer":1,"visible":true}),
    )
    .await
    .unwrap();
    assert!(manager.manual_graphics.status().1[0].running);
    let revision = command(manager.clone(), revision, json!({"action":"save","layer":1,"settings":{"source":"logo.gif","fade_in":0.3,"fade_out":0.5}})).await.unwrap();
    command(
        manager.clone(),
        revision,
        json!({"action":"visibility","layer":1,"visible":false}),
    )
    .await
    .unwrap();
    let current = manager.graphics.preparation().budget().usage();
    assert_eq!((usage.0, usage.1), (current.0, current.1));
    manager.is_alive.store(false, Ordering::SeqCst);
    manager.graphics.cancel();
    stop_runtime(&manager).await;
    manager.graphics.join(1).await;
    pool.close().await;
    tokio::fs::remove_dir_all(root).await.unwrap();
}
