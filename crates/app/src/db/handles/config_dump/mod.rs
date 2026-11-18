//! Consistent settings snapshots and atomic restoration into an unused database.
use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::{Column, Row, SqliteConnection, SqlitePool, TypeInfo, ValueRef};

mod restore;
mod tables;
#[cfg(test)]
mod tests;
mod validation;

use crate::{
    db::models::{ConfigDump, SettingsRow},
    utils::errors::ProcessError,
};

pub use restore::restore;

const FORMAT: &str = "ffplayout-settings";
const VERSION: u32 = 1;

fn invalid(message: impl Into<String>) -> ProcessError {
    ProcessError::Custom(message.into())
}

async fn schema_version(connection: &mut SqliteConnection) -> Result<i64, ProcessError> {
    Ok(
        sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(connection)
            .await?,
    )
}

async fn snapshot(connection: &mut SqliteConnection) -> Result<ConfigDump, ProcessError> {
    let mut tables = BTreeMap::new();

    for &table in tables::TABLES {
        // Identifiers come exclusively from TABLES; data is never interpolated.
        let query = format!("SELECT * FROM \"{table}\" ORDER BY rowid");
        let records = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .fetch_all(&mut *connection)
            .await?;
        let mut rows = Vec::new();

        for record in records {
            let mut row = SettingsRow::new();

            for column in record.columns() {
                let name = column.name();

                if tables::omitted(table, name) {
                    continue;
                }

                let raw = record.try_get_raw(name)?;
                let value = if raw.is_null() {
                    Value::Null
                } else {
                    match raw.type_info().name() {
                        "INTEGER" | "BOOLEAN" => Value::from(record.try_get::<i64, _>(name)?),
                        "REAL" => Value::from(record.try_get::<f64, _>(name)?),
                        "TEXT" => Value::from(record.try_get::<String, _>(name)?),
                        _ => {
                            return Err(invalid(format!(
                                "Unsupported settings type in {table}.{name}"
                            )));
                        }
                    }
                };

                row.insert(name.to_owned(), value);
            }

            rows.push(row);
        }

        tables.insert(table.to_owned(), rows);
    }

    Ok(ConfigDump {
        format: FORMAT.into(),
        version: VERSION,
        ffplayout_version: env!("CARGO_PKG_VERSION").into(),
        schema_version: schema_version(connection).await?,
        tables,
    })
}

/// Read all records within one snapshot, without resolving paths or loading media.
pub async fn export(pool: &SqlitePool) -> Result<ConfigDump, ProcessError> {
    let mut transaction = pool.begin().await?;
    let dump = snapshot(&mut transaction).await?;

    transaction.commit().await?;

    Ok(dump)
}
