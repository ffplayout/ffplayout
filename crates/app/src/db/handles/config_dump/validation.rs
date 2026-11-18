use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{Row, SqliteConnection};

use crate::{
    db::models::ConfigDump,
    utils::{config::ProgramFormat, errors::ProcessError},
};

use super::{FORMAT, VERSION, invalid, tables};

async fn columns(
    connection: &mut SqliteConnection,
    table: &str,
) -> Result<BTreeMap<String, (String, bool)>, ProcessError> {
    let records = sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA table_info(\"{table}\")"
    )))
    .fetch_all(connection)
    .await?;

    Ok(records
        .into_iter()
        .filter_map(|row| {
            let name: String = row.get("name");

            (!tables::omitted(table, &name)).then(|| {
                let required = row.get::<i64, _>("notnull") != 0 || row.get::<i64, _>("pk") != 0;

                (name, (row.get("type"), required))
            })
        })
        .collect())
}

pub(super) async fn validate(
    connection: &mut SqliteConnection,
    dump: &ConfigDump,
) -> Result<(), ProcessError> {
    if dump.format != FORMAT || dump.version != VERSION {
        return Err(invalid("Unsupported settings dump format or version"));
    }

    let expected: BTreeSet<_> = tables::TABLES
        .iter()
        .map(|table| (*table).to_owned())
        .collect();

    if dump.tables.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(invalid("Settings dump contains missing or unknown tables"));
    }

    for (table, rows) in &dump.tables {
        let columns = columns(connection, table).await?;
        let names: BTreeSet<_> = columns.keys().collect();

        for row in rows {
            if row.keys().collect::<BTreeSet<_>>() != names {
                return Err(invalid(format!("Invalid fields in settings table {table}")));
            }

            for (name, value) in row {
                let (kind, required) = &columns[name];
                let valid = if value.is_null() {
                    !required
                } else {
                    match kind.as_str() {
                        "INTEGER" | "BOOLEAN" => value.as_i64().is_some(),
                        "REAL" => value.as_f64().is_some(),
                        "TEXT" => value.is_string(),
                        _ => false,
                    }
                };

                if !valid {
                    return Err(invalid(format!("Invalid value type in {table}.{name}")));
                }

                if name == "id"
                    && value
                        .as_i64()
                        .is_some_and(|id| !(1..=i64::from(i32::MAX)).contains(&id))
                {
                    return Err(invalid(format!("Invalid settings record ID in {table}")));
                }
            }
        }
    }

    if dump.tables["channels"].is_empty() {
        return Err(invalid("Settings dump contains no channels"));
    }

    validate_structured_settings(dump)?;
    validate_relationships(dump)?;
    validate_encoding_dependencies(dump)?;

    if dump.tables["config_global"].len() != 1 || dump.tables["config_global"][0]["id"] != 1 {
        return Err(invalid(
            "Settings dump must contain exactly one global settings record",
        ));
    }

    Ok(())
}

fn validate_relationships(dump: &ConfigDump) -> Result<(), ProcessError> {
    let channel_ids: BTreeSet<_> = dump.tables["channels"]
        .iter()
        .filter_map(|row| row["id"].as_i64())
        .collect();
    let configured: BTreeSet<_> = dump.tables["config"]
        .iter()
        .filter_map(|row| row["channel_id"].as_i64())
        .collect();
    let configs: BTreeSet<_> = dump.tables["config"]
        .iter()
        .filter_map(|row| row["id"].as_i64())
        .collect();

    if channel_ids != configured || dump.tables["config"].len() != channel_ids.len() {
        return Err(invalid("Each channel must have exactly one configuration"));
    }

    for table in [
        "config_general",
        "config_mail",
        "config_notification",
        "config_logging",
        "config_processing",
        "config_audio",
        "config_playlist",
        "config_storage",
        "config_task",
        "config_source",
        "config_recording",
    ] {
        let rows = &dump.tables[table];
        let present: BTreeSet<_> = rows
            .iter()
            .filter_map(|row| row["config_id"].as_i64())
            .collect();

        if rows.len() != configs.len() || present != configs {
            return Err(invalid(format!("Incomplete configuration table {table}")));
        }
    }

    let outputs: BTreeMap<_, _> = dump.tables["config_output"]
        .iter()
        .filter_map(|row| Some((row["id"].as_i64()?, row["config_id"].as_i64()?)))
        .collect();

    if outputs.values().copied().collect::<BTreeSet<_>>() != configs {
        return Err(invalid("Each configuration must have outputs"));
    }

    for table in ["config_output", "config_recording"] {
        for row in &dump.tables[table] {
            let column = if table == "config_output" {
                "encoding_source_id"
            } else {
                "source_output_id"
            };

            if let Some(id) = row[column].as_i64()
                && outputs.get(&id) != row["config_id"].as_i64().as_ref()
            {
                return Err(invalid(
                    "Output references must remain within their channel",
                ));
            }
        }
    }

    Ok(())
}

fn parse_field<T: DeserializeOwned>(
    table: &str,
    column: &str,
    text: &str,
) -> Result<T, ProcessError> {
    serde_json::from_str(text)
        .map_err(|error| invalid(format!("Invalid settings in {table}.{column}: {error}")))
}

/// Check serialized settings without probing codecs or requiring source files on this host.
/// Column presence, nullability and scalar types have already been validated.
fn validate_structured_settings(dump: &ConfigDump) -> Result<(), ProcessError> {
    for (table, columns) in [
        (
            "config_output",
            &[
                "video_options",
                "audio_options",
                "muxer_options",
                "protocol_options",
                "metadata_options",
                "device_options",
            ][..],
        ),
        ("config_recording", &["video_options", "audio_options"][..]),
        (
            "config_source",
            &["demuxer_options", "protocol_options"][..],
        ),
        ("config_live_input", &["options", "demuxer_options"][..]),
        ("config_output_audio", &["encoder_options"][..]),
    ] {
        for row in &dump.tables[table] {
            for &column in columns {
                let text = row[column].as_str().unwrap();

                // Older seed records use an empty string for default encoder options.
                if !text.is_empty() {
                    parse_field::<BTreeMap<String, String>>(table, column, text)?;
                }
            }
        }
    }

    for row in &dump.tables["config"] {
        if let Some(text) = row["program_format"].as_str() {
            let program = parse_field::<ProgramFormat>("config", "program_format", text)?;

            program.validate().map_err(|error| {
                invalid(format!(
                    "Invalid settings in config.program_format: {error}"
                ))
            })?;
        }
    }

    for (table, column) in [
        ("config_graphics", "settings"),
        ("config_output_audio", "channel_map"),
    ] {
        for row in &dump.tables[table] {
            parse_field::<BTreeMap<String, Value>>(table, column, row[column].as_str().unwrap())?;
        }
    }

    Ok(())
}

fn validate_encoding_dependencies(dump: &ConfigDump) -> Result<(), ProcessError> {
    let outputs: BTreeMap<_, _> = dump.tables["config_output"]
        .iter()
        .map(|row| (row["id"].as_i64().unwrap(), row))
        .collect();
    let mut checked = BTreeSet::new();

    for &start in outputs.keys() {
        let mut path = BTreeSet::new();
        let mut current = start;

        while !checked.contains(&current) {
            if !path.insert(current) {
                return Err(invalid("Output encoding sources must not contain a cycle"));
            }

            let row = outputs[&current];

            if row["share_encoding"].as_i64() != Some(1) {
                break;
            }

            let Some(source) = row["encoding_source_id"].as_i64() else {
                break;
            };

            if !outputs.contains_key(&source) {
                return Err(invalid(
                    "Shared encoding requires an existing source output",
                ));
            }

            current = source;
        }

        checked.extend(path);
    }

    Ok(())
}
