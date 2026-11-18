use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type SettingsRow = BTreeMap<String, Value>;

/// Named settings records, never SQL statements or driver-specific migration state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDump {
    pub format: String,
    pub version: u32,
    /// Application version that created the export; informational only.
    #[serde(default)]
    pub ffplayout_version: String,
    /// Source migration revision for diagnostics, never an import compatibility gate.
    pub schema_version: i64,
    pub tables: BTreeMap<String, Vec<SettingsRow>>,
}
