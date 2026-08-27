use sqlx::{
    Executor, Sqlite,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::TextPreset, utils::errors::ProcessError};

pub async fn select_presets(pool: &SqlitePool, id: i32) -> Result<Vec<TextPreset>, ProcessError> {
    const QUERY: &str = "SELECT preset.*, config.channel_id
        FROM config_text_presets preset
        JOIN config ON config.id = preset.config_id
        WHERE config.channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_all(pool).await?;

    Ok(result)
}

pub async fn select_preset(
    pool: &SqlitePool,
    channel_id: i32,
    preset_id: i32,
) -> Result<TextPreset, ProcessError> {
    const QUERY: &str = "SELECT preset.*, config.channel_id
        FROM config_text_presets preset
        JOIN config ON config.id = preset.config_id
        WHERE config.channel_id = $1 AND preset.id = $2";

    Ok(sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(preset_id)
        .fetch_one(pool)
        .await?)
}

pub async fn set_persistent_preset(
    pool: &SqlitePool,
    channel_id: i32,
    preset_id: Option<i32>,
) -> Result<(), ProcessError> {
    let mut transaction = pool.begin().await?;
    let config_id: i32 = sqlx::query_scalar("SELECT id FROM config WHERE channel_id = $1")
        .bind(channel_id)
        .fetch_one(&mut *transaction)
        .await?;
    sqlx::query("UPDATE config_text_presets SET persistent = 0 WHERE config_id = $1")
        .bind(config_id)
        .execute(&mut *transaction)
        .await?;

    if let Some(preset_id) = preset_id {
        let result = sqlx::query(
            "UPDATE config_text_presets SET persistent = 1 WHERE config_id = $1 AND id = $2",
        )
        .bind(config_id)
        .bind(preset_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(ProcessError::Custom(format!(
                "Text preset {preset_id} does not belong to channel {channel_id}"
            )));
        }
    }

    transaction.commit().await?;
    Ok(())
}

pub async fn update_preset(
    pool: &SqlitePool,
    channel_id: i32,
    id: &i32,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE config_text_presets SET name = $1, text = $2, use_filename = $3,
        font_family = $4, font_weight = $5, filename_regex = $6, position_x = $7,
        position_y = $8, font_size = $9, line_spacing = $10, text_color = $11,
        text_opacity = $12, background_enabled = $13, background_color = $14,
        background_opacity = $15, background_padding = $16, opacity = $17,
        scroll_direction = $18, scroll_speed = $19, scroll_repeat = $20,
        fade_in_seconds = $21, fade_out_seconds = $22 WHERE id = $23
        AND config_id = (SELECT id FROM config WHERE channel_id = $24)";

    let result = sqlx::query(QUERY)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.use_filename)
        .bind(preset.font_family)
        .bind(preset.font_weight)
        .bind(preset.filename_regex)
        .bind(preset.position_x)
        .bind(preset.position_y)
        .bind(preset.font_size)
        .bind(preset.line_spacing)
        .bind(preset.text_color)
        .bind(preset.text_opacity)
        .bind(preset.background_enabled)
        .bind(preset.background_color)
        .bind(preset.background_opacity)
        .bind(preset.background_padding)
        .bind(preset.opacity)
        .bind(preset.scroll_direction)
        .bind(preset.scroll_speed)
        .bind(preset.scroll_repeat)
        .bind(preset.fade_in_seconds)
        .bind(preset.fade_out_seconds)
        .bind(id)
        .bind(channel_id)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn insert_preset(
    pool: &SqlitePool,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "INSERT INTO config_text_presets (
        config_id, name, text, use_filename, font_family, font_weight, filename_regex,
        position_x, position_y, font_size, line_spacing, text_color, text_opacity,
        background_enabled, background_color, background_opacity, background_padding,
        opacity, scroll_direction, scroll_speed, scroll_repeat, fade_in_seconds, fade_out_seconds
    ) VALUES(
        (SELECT id FROM config WHERE channel_id = $1), $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
        $16, $17, $18, $19, $20, $21, $22, $23
    )";

    let result = sqlx::query(QUERY)
        .bind(preset.channel_id)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.use_filename)
        .bind(preset.font_family)
        .bind(preset.font_weight)
        .bind(preset.filename_regex)
        .bind(preset.position_x)
        .bind(preset.position_y)
        .bind(preset.font_size)
        .bind(preset.line_spacing)
        .bind(preset.text_color)
        .bind(preset.text_opacity)
        .bind(preset.background_enabled)
        .bind(preset.background_color)
        .bind(preset.background_opacity)
        .bind(preset.background_padding)
        .bind(preset.opacity)
        .bind(preset.scroll_direction)
        .bind(preset.scroll_speed)
        .bind(preset.scroll_repeat)
        .bind(preset.fade_in_seconds)
        .bind(preset.fade_out_seconds)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn new_channel_presets<'e, E>(
    executor: E,
    config_id: i32,
) -> Result<SqliteQueryResult, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    const QUERY: &str = "INSERT INTO config_text_presets (
        name, text, use_filename, position_x, position_y, background_enabled,
        scroll_direction, fade_in_seconds, fade_out_seconds, config_id
    ) VALUES
        ('Default', 'Welcome to ffplayout messenger!', 0, 'center', 'center', 0, 'none', 0.0, 0.0, $1),
        ('Bottom Text fade in', 'The upcoming event will be delayed by a few minutes.', 0, 'center', 'end:72', 1, 'none', 1.0, 1.0, $1),
        ('Scrolling Text', 'We have a very important announcement to make.', 0, 'center', 'end:72', 1, 'right_to_left', 0.0, 0.0, $1),
        ('Filename overlay', '', 1, 'center', 'end:72', 1, 'none', 0.0, 0.0, $1);";

    let result = sqlx::query(QUERY).bind(config_id).execute(executor).await?;

    Ok(result)
}

pub async fn delete_preset(
    pool: &SqlitePool,
    channel_id: i32,
    id: &i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM config_text_presets WHERE id = $1
        AND config_id = (SELECT id FROM config WHERE channel_id = $2);";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel_id)
        .execute(pool)
        .await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::handles::db_migrate;

    #[tokio::test]
    async fn preset_crud_uses_the_owning_config() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();

        let presets = select_presets(&pool, 1).await.unwrap();
        assert_eq!(presets.len(), 4);
        assert!(presets.iter().all(|preset| preset.channel_id == 1));

        let mut preset = TextPreset {
            name: "CRUD preset".to_string(),
            text: "Initial".to_string(),
            ..TextPreset::default()
        };
        assert_eq!(
            insert_preset(&pool, preset.clone())
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        let id: i32 =
            sqlx::query_scalar("SELECT id FROM config_text_presets WHERE name = 'CRUD preset'")
                .fetch_one(&pool)
                .await
                .unwrap();
        preset.text = "Updated".to_string();

        assert_eq!(
            update_preset(&pool, 999, &id, preset.clone())
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(
            update_preset(&pool, 1, &id, preset)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        assert_eq!(select_preset(&pool, 1, id).await.unwrap().text, "Updated");
        set_persistent_preset(&pool, 1, Some(id)).await.unwrap();
        assert!(select_preset(&pool, 1, id).await.unwrap().persistent);
        assert!(
            set_persistent_preset(&pool, 1, Some(i32::MAX))
                .await
                .is_err()
        );
        assert!(select_preset(&pool, 1, id).await.unwrap().persistent);
        set_persistent_preset(&pool, 1, None).await.unwrap();
        assert!(!select_preset(&pool, 1, id).await.unwrap().persistent);

        assert_eq!(
            delete_preset(&pool, 999, &id)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(
            delete_preset(&pool, 1, &id).await.unwrap().rows_affected(),
            1
        );
        assert!(select_preset(&pool, 1, id).await.is_err());
    }
}
