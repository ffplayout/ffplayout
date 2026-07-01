use sqlx::{
    Row,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{AdvancedConfig, db::models::AdvancedConfiguration, utils::errors::ProcessError};

pub async fn insert_advanced_configuration(
    pool: &SqlitePool,
    channel_id: i32,
    adv_id: Option<i32>,
    config: AdvancedConfig,
) -> Result<i32, ProcessError> {
    const QUERY_INSERT: &str =
        "INSERT INTO advanced_configurations (channel_id, decoder_input_param, decoder_output_param, encoder_input_param,
            ingest_input_param, filter_deinterlace, filter_pad_video, filter_fps, filter_scale, filter_set_dar,
            filter_fade_in, filter_fade_out, filter_logo, filter_overlay_logo_scale, filter_overlay_logo_fade_in,
            filter_overlay_logo_fade_out, filter_overlay_logo, filter_tpad, filter_drawtext_from_file,
            filter_drawtext_from_zmq, filter_aevalsrc, filter_afade_in, filter_afade_out, filter_apad,
            filter_volume, filter_split, name)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27) RETURNING id";

    const QUERY_UPDATE: &str = "UPDATE channels SET advanced_id = $2 WHERE id = $1";

    let advanced_id: i32 = sqlx::query(QUERY_INSERT)
        .bind(channel_id)
        .bind(config.decoder.input_param)
        .bind(config.decoder.output_param)
        .bind(config.encoder.input_param)
        .bind(config.ingest.input_param)
        .bind(config.filter.deinterlace)
        .bind(config.filter.pad_video)
        .bind(config.filter.fps)
        .bind(config.filter.scale)
        .bind(config.filter.set_dar)
        .bind(config.filter.fade_in)
        .bind(config.filter.fade_out)
        .bind(config.filter.logo)
        .bind(config.filter.overlay_logo_scale)
        .bind(config.filter.overlay_logo_fade_in)
        .bind(config.filter.overlay_logo_fade_out)
        .bind(config.filter.overlay_logo)
        .bind(config.filter.tpad)
        .bind(config.filter.drawtext_from_file)
        .bind(config.filter.drawtext_from_zmq)
        .bind(config.filter.aevalsrc)
        .bind(config.filter.afade_in)
        .bind(config.filter.afade_out)
        .bind(config.filter.apad)
        .bind(config.filter.volume)
        .bind(config.filter.split)
        .bind(config.name)
        .fetch_one(pool)
        .await?
        .get("id");

    let a_id = adv_id.unwrap_or(advanced_id);

    sqlx::query(QUERY_UPDATE)
        .bind(channel_id)
        .bind(a_id)
        .execute(pool)
        .await?;

    Ok(advanced_id)
}

pub async fn update_advanced_configuration(
    pool: &SqlitePool,
    id: i32,
    config: AdvancedConfig,
) -> Result<(), ProcessError> {
    const QUERY_ADV: &str = "UPDATE advanced_configurations SET decoder_input_param = $2, decoder_output_param = $3,
        encoder_input_param = $4, ingest_input_param = $5, filter_deinterlace = $6, filter_pad_video = $7, filter_fps = $8,
        filter_scale = $9, filter_set_dar = $10, filter_fade_in = $11, filter_fade_out = $12, filter_logo = $13,
        filter_overlay_logo_scale = $14, filter_overlay_logo_fade_in = $15, filter_overlay_logo_fade_out = $16,
        filter_overlay_logo = $17, filter_tpad = $18, filter_drawtext_from_file = $19, filter_drawtext_from_zmq = $20,
        filter_aevalsrc = $21, filter_afade_in = $22, filter_afade_out = $23, filter_apad = $24, filter_volume = $25, filter_split = $26, name = $27
        WHERE id = $1";
    const QUERY_CHL: &str = "UPDATE channels set advanced_id = $2 WHERE id = $1;";

    sqlx::query(QUERY_ADV)
        .bind(config.id)
        .bind(config.decoder.input_param)
        .bind(config.decoder.output_param)
        .bind(config.encoder.input_param)
        .bind(config.ingest.input_param)
        .bind(config.filter.deinterlace)
        .bind(config.filter.pad_video)
        .bind(config.filter.fps)
        .bind(config.filter.scale)
        .bind(config.filter.set_dar)
        .bind(config.filter.fade_in)
        .bind(config.filter.fade_out)
        .bind(config.filter.logo)
        .bind(config.filter.overlay_logo_scale)
        .bind(config.filter.overlay_logo_fade_in)
        .bind(config.filter.overlay_logo_fade_out)
        .bind(config.filter.overlay_logo)
        .bind(config.filter.tpad)
        .bind(config.filter.drawtext_from_file)
        .bind(config.filter.drawtext_from_zmq)
        .bind(config.filter.aevalsrc)
        .bind(config.filter.afade_in)
        .bind(config.filter.afade_out)
        .bind(config.filter.apad)
        .bind(config.filter.volume)
        .bind(config.filter.split)
        .bind(config.name)
        .execute(pool)
        .await?;

    sqlx::query(QUERY_CHL)
        .bind(id)
        .bind(config.id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn select_advanced_configuration(
    pool: &SqlitePool,
    channel: i32,
) -> Result<AdvancedConfiguration, ProcessError> {
    const QUERY: &str = "SELECT adv.id, adv.channel_id, adv.decoder_input_param, adv.decoder_output_param, adv.encoder_input_param,
        adv.ingest_input_param, adv.filter_deinterlace, adv.filter_pad_video, adv.filter_fps,adv.filter_scale, adv.filter_set_dar,
        adv.filter_fade_in, adv.filter_fade_out, adv.filter_overlay_logo_scale, adv.filter_overlay_logo_fade_in, adv.filter_overlay_logo_fade_out,
        adv.filter_overlay_logo, adv.filter_tpad, adv.filter_drawtext_from_file, adv.filter_drawtext_from_zmq, adv.filter_aevalsrc,
        adv.filter_afade_in, adv.filter_afade_out, adv.filter_apad, adv.filter_volume, adv.filter_split, adv.filter_logo, adv.name
        FROM advanced_configurations adv left join channels ch on ch.advanced_id = adv.id WHERE ch.id = $1";

    let result = sqlx::query_as(QUERY)
        .bind(channel)
        .fetch_optional(pool)
        .await?
        .unwrap_or_default();

    Ok(result)
}

pub async fn select_related_advanced_configuration(
    pool: &SqlitePool,
    channel: i32,
) -> Result<Vec<AdvancedConfiguration>, ProcessError> {
    const QUERY: &str = "SELECT * FROM advanced_configurations WHERE channel_id = $1;";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(pool).await?;

    Ok(result)
}

pub async fn delete_advanced_configuration(
    pool: &SqlitePool,
    id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM advanced_configurations WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}
