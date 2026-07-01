use sqlx::{
    Row,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::Output, utils::errors::ProcessError};

pub async fn select_outputs(pool: &SqlitePool, channel: i32) -> Result<Vec<Output>, ProcessError> {
    const QUERY: &str = "SELECT * FROM outputs WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_output(
    pool: &SqlitePool,
    channel_id: i32,
    output: &Output,
) -> Result<i32, ProcessError> {
    const QUERY: &str =
        "INSERT INTO outputs (channel_id, name, parameters) VALUES($1, $2, $3) RETURNING id";

    let output_id = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(&output.name)
        .bind(&output.parameters)
        .fetch_one(pool)
        .await?
        .get("id");

    Ok(output_id)
}

pub async fn update_output(
    pool: &SqlitePool,
    id: i32,
    channel_id: i32,
    parameters: &str,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE outputs SET parameters = $3 WHERE id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel_id)
        .bind(parameters)
        .execute(pool)
        .await?;

    Ok(result)
}
