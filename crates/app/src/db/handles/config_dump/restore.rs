use serde_json::Value;
use sqlx::{SqliteConnection, SqlitePool};

use crate::{
    db::models::{ConfigDump, SettingsRow},
    utils::errors::ProcessError,
};

use super::{export, invalid, snapshot, tables, validation::validate};

async fn insert_rows(
    connection: &mut SqliteConnection,
    table: &str,
    rows: &[SettingsRow],
) -> Result<(), ProcessError> {
    for row in rows {
        // validate() matched every key against the local schema and table
        // names against TABLES. Only those identifiers enter SQL; values bind.
        let names: Vec<_> = row.keys().map(|name| format!("\"{name}\"")).collect();
        let sql = format!(
            "INSERT INTO \"{table}\" ({}) VALUES ({})",
            names.join(","),
            vec!["?"; row.len()].join(",")
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));

        for value in row.values() {
            query = match value {
                Value::Null => query.bind(Option::<String>::None),
                Value::String(text) => query.bind(text),
                Value::Number(number) if number.is_i64() => query.bind(number.as_i64()),
                Value::Number(number) => query.bind(number.as_f64()),
                _ => return Err(invalid("Invalid settings value")),
            };
        }

        query.execute(&mut *connection).await?;
    }

    Ok(())
}

/// Restore only into an untouched, migrated database. Every write is transactional.
pub async fn restore(pool: &SqlitePool, mut dump: ConfigDump) -> Result<(), ProcessError> {
    let baseline = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    super::super::db_migrate(&baseline).await?;

    let pristine = export(&baseline).await?;
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;

    validate(&mut transaction, &dump).await?;

    if snapshot(&mut transaction).await?.tables != pristine.tables {
        return Err(invalid(
            "Settings import requires a fresh, unconfigured database",
        ));
    }

    if dump.tables["auth_roles"] != pristine.tables["auth_roles"] {
        return Err(invalid("Unsupported authorization roles in settings dump"));
    }

    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM channels")
        .execute(&mut *transaction)
        .await?;

    let secret: String = sqlx::query_scalar("SELECT secret FROM config_global WHERE id = 1")
        .fetch_one(&mut *transaction)
        .await?;

    sqlx::query("DELETE FROM config_global")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM auth_roles")
        .execute(&mut *transaction)
        .await?;
    dump.tables.get_mut("config_global").unwrap()[0].insert("secret".into(), Value::from(secret));

    for &table in tables::TABLES {
        insert_rows(&mut transaction, table, &dump.tables[table]).await?;
    }

    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *transaction)
        .await?;

    if !broken.is_empty() {
        return Err(invalid("Settings dump contains invalid relationships"));
    }

    transaction.commit().await?;
    baseline.close().await;

    Ok(())
}
