use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use ffplayout::db::{
    handles::{self, config_dump},
    models::ConfigDump,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

fn run(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ffplayout"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test]
async fn json_cli_export_import_preserves_settings_and_protects_existing_database() {
    let root = std::env::temp_dir().join(format!("ffplayout-json-cli-{}", uuid::Uuid::new_v4()));

    fs::create_dir(&root).unwrap();

    let source = root.join("source.db");
    let target = root.join("restored.db");
    let source_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&source)
                .create_if_missing(true),
        )
        .await
        .unwrap();

    handles::db_migrate(&source_pool).await.unwrap();
    sqlx::query("UPDATE config_global SET smtp_password='private-cli-secret'")
        .execute(&source_pool)
        .await
        .unwrap();
    source_pool.close().await;

    let dumped = run(&root, &["--db", source.to_str().unwrap(), "--dump-config"]);

    assert!(dumped.status.success(), "{dumped:?}");
    assert!(!String::from_utf8_lossy(&dumped.stdout).contains("private-cli-secret"));

    let file = root.join("ffplayout.json");
    let expected: ConfigDump = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();

    assert!(expected.tables["config_global"][0]["smtp_password"] == "private-cli-secret");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::set_permissions(&file, fs::Permissions::from_mode(0o666)).unwrap();

        let repeated_dump = run(&root, &["--db", source.to_str().unwrap(), "--dump-config"]);

        assert!(repeated_dump.status.success(), "{repeated_dump:?}");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let imported = run(
        &root,
        &[
            "--db",
            target.to_str().unwrap(),
            "--import-config",
            file.to_str().unwrap(),
        ],
    );

    assert!(imported.status.success(), "{imported:?}");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&target))
        .await
        .unwrap();

    assert_eq!(config_dump::export(&pool).await.unwrap(), expected);
    pool.close().await;

    let repeated = run(
        &root,
        &[
            "--db",
            target.to_str().unwrap(),
            "--import-config",
            file.to_str().unwrap(),
        ],
    );

    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("fresh"));

    let selected = run(
        &root,
        &[
            "--db",
            source.to_str().unwrap(),
            "--dump-config",
            "--channel",
            "1",
        ],
    );

    assert!(!selected.status.success());
    assert!(String::from_utf8_lossy(&selected.stderr).contains("cannot be used with"));
    assert!(!root.join("ffplayout_1.json").exists());
    fs::write(root.join("old.toml"), "[general]\nstop_threshold=11\n").unwrap();

    let invalid = run(
        &root,
        &[
            "--db",
            root.join("invalid.db").to_str().unwrap(),
            "--import-config",
            root.join("old.toml").to_str().unwrap(),
        ],
    );

    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Invalid JSON settings dump"));
    assert!(!String::from_utf8_lossy(&invalid.stderr).contains("panicked"));
    fs::remove_dir_all(root).unwrap();
}
