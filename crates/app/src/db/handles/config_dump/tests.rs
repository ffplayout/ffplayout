use std::collections::BTreeSet;

use super::*;

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    super::super::db_migrate(&pool).await.unwrap();

    pool
}

#[tokio::test]
async fn full_settings_roundtrip() {
    let source = pool().await;
    let target = pool().await;
    let original = export(&source).await.unwrap();

    restore(&target, original.clone()).await.unwrap();
    assert_eq!(export(&target).await.unwrap(), original);
}

async fn populated() -> SqlitePool {
    let pool = pool().await;

    sqlx::raw_sql(
        "UPDATE config_global SET smtp_server='smtp.private.test', smtp_password='private-password', notification_token='private-token', setup_completed=1;
         UPDATE channels SET name='Original channel', timezone='Europe/Berlin', active=1;
         UPDATE config_processing SET logo='missing-but-configured.png', add_logo=1, graphics_cache_mib=512;
         UPDATE config_recording SET enabled=1, source_output_id=(SELECT id FROM config_output WHERE name='hls' LIMIT 1), path='/recordings/custom';
         UPDATE config_output SET encoding_source_id=(SELECT id FROM config_output WHERE name='hls' LIMIT 1), share_encoding=1 WHERE name='stream';
         UPDATE config_source SET demuxer_options='{\"probesize\":\"1000000\"}';
         UPDATE config_text_presets SET text='Custom preset', font_size=48 WHERE id=(SELECT MIN(id) FROM config_text_presets);
         UPDATE config_live_input SET name='Custom RTMP', idle_timeout_ms=4321, auto_takeover=0;
         INSERT INTO config_live_input(id,config_id,name,backend,identifier,takeover_mode) VALUES(144,1,'Custom SRT','srt','srt://0.0.0.0:9999','connection');
         INSERT INTO auth_user(id,mail,username,password,role_id,two_factor) VALUES(67,'admin@test.invalid','restored-admin','stored-password-hash',1,1);
         INSERT INTO auth_user_channels(id,user_id,channel_id) VALUES(81,67,1);
         INSERT INTO config_graphics_state(channel_id,revision) VALUES(1,12);
         INSERT INTO config_graphics(channel_id,layer_id,name,settings,running,visible) VALUES(1,42,'Lower third','{\"source\":\"lower-third.png\"}',1,0);
         INSERT INTO config_graphics_schedule(channel_id,layer_id,position,appointment) VALUES(1,42,0,'{\"enabled\":true,\"date\":\"2026-11-28\",\"start_time\":\"10:00\",\"duration_seconds\":30}');
         INSERT INTO config_output_audio(id,output_id,position,title) SELECT 92,id,0,'Custom audio' FROM config_output WHERE name='hls' LIMIT 1;
         INSERT INTO auth_refresh_tokens(jti,family_id,user_id,expires_at,created_at) VALUES('omitted-session','omitted-family',67,9999999999,1);"
    ).execute(&pool).await.unwrap();

    pool
}

#[tokio::test]
async fn restores_every_persistent_table_with_sparse_ids_and_relationships() {
    let source = populated().await;

    super::super::create_channel_records(
        &source,
        crate::db::models::Channel {
            name: "Another channel".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Keep gaps and non-default IDs in every relationship, including encoder links
    // that are not represented by a database foreign key.
    let mut transaction = source.begin().await.unwrap();

    sqlx::raw_sql(
        "PRAGMA defer_foreign_keys = ON;
         UPDATE channels SET id=id+1000;
         UPDATE config_graphics SET channel_id=channel_id+1000;
         UPDATE config_graphics_state SET channel_id=channel_id+1000;
         UPDATE config_graphics_schedule SET channel_id=channel_id+1000;
         UPDATE config SET id=id+1000;
         UPDATE auth_user SET id=id+1000;
         UPDATE config_text_presets SET id=id+1000;
         UPDATE config_output SET id=id+1000;
         UPDATE config_output SET encoding_source_id=encoding_source_id+1000
            WHERE encoding_source_id IS NOT NULL;",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let target = pool().await;
    let target_secret: String = sqlx::query_scalar("SELECT secret FROM config_global")
        .fetch_one(&target)
        .await
        .unwrap();
    let dump = export(&source).await.unwrap();

    let serialized = serde_json::to_vec(&dump).unwrap();
    let imported = serde_json::from_slice(&serialized).unwrap();

    restore(&target, imported).await.unwrap();
    assert_eq!(export(&target).await.unwrap(), dump);
    assert!(
        super::super::select_global(&target)
            .await
            .unwrap()
            .setup_completed
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT secret FROM config_global")
            .fetch_one(&target)
            .await
            .unwrap(),
        target_secret
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM auth_refresh_tokens")
            .fetch_one(&target)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT password FROM auth_user")
            .fetch_one(&target)
            .await
            .unwrap(),
        "stored-password-hash"
    );
}

#[tokio::test]
async fn rejects_invalid_dumps_and_rolls_back_all_tables() {
    let source = populated().await;
    let valid = export(&source).await.unwrap();

    for kind in 0..14 {
        let target = pool().await;
        let before = export(&target).await.unwrap();
        let mut dump = valid.clone();

        match kind {
            0 => dump.version += 1,
            1 => {
                dump.tables.remove("config_recording");
            }
            2 => {
                dump.tables.get_mut("config_output").unwrap()[0]
                    .insert("encoding_source_id".into(), Value::from(999999));
            }
            3 => {
                dump.tables.get_mut("config_graphics").unwrap()[0]
                    .insert("settings".into(), Value::from("invalid JSON"));
            }
            4 => {
                dump.tables.get_mut("config_global").unwrap()[0]
                    .insert("secret".into(), Value::from("must-not-import"));
            }
            5 => {
                dump.tables.get_mut("config_audio").unwrap().clear();
            }
            6 => {
                dump.tables.get_mut("channels").unwrap()[0]
                    .insert("id".into(), Value::from("invalid id"));
            }
            7 => {
                dump.tables.get_mut("auth_user").unwrap()[0]
                    .insert("role_id".into(), Value::from(999));
            }
            8 => {
                let duplicate = dump.tables["channels"][0].clone();

                dump.tables.get_mut("channels").unwrap().push(duplicate);
            }
            9 => {
                dump.tables.get_mut("config_general").unwrap()[0]
                    .insert("unknown\";DELETE FROM channels;--".into(), Value::from(1));
            }
            10 => {
                dump.tables.get_mut("config_recording").unwrap()[0]
                    .insert("config_id".into(), Value::Null);
            }
            11 => {
                dump.tables.get_mut("auth_user").unwrap()[0].insert("id".into(), Value::from(-1));
            }
            12 => {
                dump.tables.get_mut("auth_user_channels").unwrap()[0]
                    .insert("user_id".into(), Value::from(999999));
            }
            _ => {
                let id = Value::from(i64::from(i32::MAX) + 1);

                dump.tables.get_mut("auth_user").unwrap()[0].insert("id".into(), id.clone());
                dump.tables.get_mut("auth_user_channels").unwrap()[0].insert("user_id".into(), id);
            }
        }

        assert!(
            restore(&target, dump).await.is_err(),
            "accepted invalid case {kind}"
        );
        assert_eq!(
            export(&target).await.unwrap(),
            before,
            "partial writes in case {kind}"
        );
    }
}

#[tokio::test]
async fn refuses_existing_installations_even_without_users() {
    let source = populated().await;
    let dump = export(&source).await.unwrap();
    let target = pool().await;

    sqlx::query("UPDATE config_audio SET volume=0.4")
        .execute(&target)
        .await
        .unwrap();

    let before = export(&target).await.unwrap();

    assert!(
        restore(&target, dump)
            .await
            .unwrap_err()
            .to_string()
            .contains("fresh")
    );
    assert_eq!(export(&target).await.unwrap(), before);
}

#[tokio::test]
async fn table_allowlist_covers_schema_except_sessions_and_migration_state() {
    let pool = pool().await;
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&pool).await.unwrap();
    let actual: BTreeSet<_> = tables
        .iter()
        .filter(|table| !matches!(table.as_str(), "_sqlx_migrations" | "auth_refresh_tokens"))
        .map(String::as_str)
        .collect();

    assert_eq!(actual, tables::TABLES.iter().copied().collect());
}

#[tokio::test]
async fn concurrent_imports_cannot_replace_an_already_restored_installation() {
    let source = populated().await;
    let target = pool().await;
    let dump = export(&source).await.unwrap();
    let (first, second) = tokio::join!(restore(&target, dump.clone()), restore(&target, dump));

    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "exactly one import must commit"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT username FROM auth_user")
            .fetch_one(&target)
            .await
            .unwrap(),
        "restored-admin"
    );
}

#[tokio::test]
async fn rejects_invalid_structured_settings_and_encoding_cycles() {
    let source = populated().await;
    let valid = export(&source).await.unwrap();

    for (table, column, text) in [
        ("config", "program_format", "invalid JSON"),
        (
            "config",
            "program_format",
            r#"{"width":"invalid","height":1080,"fps":25}"#,
        ),
        ("config_source", "demuxer_options", r#"{"probesize":42}"#),
        ("config_live_input", "options", "[]"),
        ("config_recording", "audio_options", "null"),
        ("config_output", "muxer_options", "invalid JSON"),
        ("config_output_audio", "encoder_options", "[]"),
        ("config_graphics", "settings", "[]"),
        ("config_output_audio", "channel_map", "null"),
    ] {
        let target = pool().await;
        let before = export(&target).await.unwrap();
        let mut dump = valid.clone();

        dump.tables.get_mut(table).unwrap()[0].insert(column.into(), Value::from(text));
        assert!(
            restore(&target, dump)
                .await
                .unwrap_err()
                .to_string()
                .contains(&format!("{table}.{column}"))
        );
        assert_eq!(export(&target).await.unwrap(), before);
    }

    for indirect in [false, true] {
        let target = pool().await;
        let before = export(&target).await.unwrap();
        let mut dump = valid.clone();
        let outputs = dump.tables.get_mut("config_output").unwrap();
        let first = outputs[0]["id"].clone();
        let second = outputs[1]["id"].clone();

        outputs[0].insert("share_encoding".into(), Value::from(1));
        outputs[0].insert(
            "encoding_source_id".into(),
            if indirect { second } else { first.clone() },
        );

        if indirect {
            outputs[1].insert("share_encoding".into(), Value::from(1));
            outputs[1].insert("encoding_source_id".into(), first);
        }

        assert!(
            restore(&target, dump)
                .await
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
        assert_eq!(export(&target).await.unwrap(), before);
    }
}

#[tokio::test]
async fn restores_settings_exported_before_ingest_forwarding_and_graphics_schedules() {
    let source = populated().await;
    let target = pool().await;
    let mut expected = export(&source).await.unwrap();

    expected
        .tables
        .get_mut("config_graphics_schedule")
        .unwrap()
        .clear();

    let mut old_dump = expected.clone();

    old_dump.tables.remove("config_graphics_schedule");

    for output in old_dump.tables.get_mut("config_output").unwrap() {
        output.remove("forward_listener_id");
        output.remove("forward_mode");
    }

    restore(&target, old_dump).await.unwrap();

    assert_eq!(export(&target).await.unwrap().tables, expected.tables);
}

#[tokio::test]
async fn import_compatibility_is_independent_of_migration_numbering() {
    let source = populated().await;
    let target = pool().await;
    let mut dump = export(&source).await.unwrap();

    // An older installation exported the same settings under migration revision 8.
    dump.schema_version = 8;
    dump.ffplayout_version = "3.0.0".into();

    // Simulate squashing the target migrations to revision 1 without schema changes.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version != 1")
        .execute(&target)
        .await
        .unwrap();
    restore(&target, dump.clone()).await.unwrap();

    let restored = export(&target).await.unwrap();

    assert_eq!(restored.schema_version, 1);
    assert_eq!(restored.tables, dump.tables);

    // Neither an incompatible format nor incompatible fields may bypass validation.
    let fresh = pool().await;
    let before = export(&fresh).await.unwrap();

    dump.version += 1;
    assert!(restore(&fresh, dump.clone()).await.is_err());
    dump.version -= 1;
    dump.tables.get_mut("config").unwrap()[0].remove("program_format");
    assert!(restore(&fresh, dump).await.is_err());
    assert_eq!(export(&fresh).await.unwrap(), before);
    assert!(
        restore(&target, before)
            .await
            .unwrap_err()
            .to_string()
            .contains("fresh")
    );
}

#[tokio::test]
async fn exports_application_version_and_accepts_dumps_without_it() {
    let source = pool().await;
    let target = pool().await;
    let dump = export(&source).await.unwrap();

    assert_eq!(dump.ffplayout_version, env!("CARGO_PKG_VERSION"));

    let mut json = serde_json::to_value(&dump).unwrap();

    json.as_object_mut().unwrap().remove("ffplayout_version");

    let older_dump: ConfigDump = serde_json::from_value(json).unwrap();

    assert!(older_dump.ffplayout_version.is_empty());
    restore(&target, older_dump).await.unwrap();
    assert_eq!(export(&target).await.unwrap().tables, dump.tables);
}

#[tokio::test]
async fn validates_program_dimensions_and_frame_rate_before_import() {
    use crate::utils::config::{PlayoutConfig, ProgramFormat};

    let source = populated().await;
    let valid = export(&source).await.unwrap();

    for (width, height, fps) in [
        (0, 1080, 25.0),
        (1920, 0, 25.0),
        (1920, 1080, 0.0),
        (1920, 1080, -25.0),
        (1920, 1080, 0.5),
        (1920, 1080, f64::from(u32::MAX) + 1.0),
    ] {
        let program = ProgramFormat { width, height, fps };
        let target = pool().await;
        let before = export(&target).await.unwrap();
        let mut dump = valid.clone();
        let config = PlayoutConfig {
            program: Some(program.clone()),
            ..Default::default()
        };

        dump.tables.get_mut("config").unwrap()[0].insert(
            "program_format".into(),
            Value::from(serde_json::to_string(&program).unwrap()),
        );
        assert_eq!(
            config.validate_outputs().unwrap_err(),
            "invalid program format"
        );
        assert!(
            restore(&target, dump)
                .await
                .unwrap_err()
                .to_string()
                .contains("config.program_format")
        );
        assert_eq!(export(&target).await.unwrap(), before);
    }

    for (width, height, fps) in [
        (1, 1, 1.0),
        (1920, 1080, 29.97),
        (1920, 1080, f64::from(u32::MAX)),
    ] {
        let program = ProgramFormat { width, height, fps };
        let target = pool().await;
        let mut dump = valid.clone();

        dump.tables.get_mut("config").unwrap()[0].insert(
            "program_format".into(),
            Value::from(serde_json::to_string(&program).unwrap()),
        );
        restore(&target, dump.clone()).await.unwrap();
        assert_eq!(export(&target).await.unwrap(), dump);
    }

    // Non-finite values cannot appear in JSON, but runtime validation still rejects them.
    for fps in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let program = ProgramFormat {
            width: 1920,
            height: 1080,
            fps,
        };

        assert!(program.validate().is_err());
    }
}
