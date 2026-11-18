//! Explicit export boundary. New persistent tables must be added deliberately.
pub(super) const TABLES: &[&str] = &[
    "config_global",
    "auth_roles",
    "channels",
    "auth_user",
    "auth_user_channels",
    "config",
    "config_general",
    "config_mail",
    "config_notification",
    "config_logging",
    "config_processing",
    "config_audio",
    "config_playlist",
    "config_storage",
    "config_text_presets",
    "config_task",
    "config_source",
    "config_live_input",
    "config_output",
    "config_output_audio",
    "config_recording",
    "config_graphics_state",
    "config_graphics",
];

pub(super) fn omitted(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("config_global", "secret") | ("channels", "last_date" | "time_shift")
    )
}
