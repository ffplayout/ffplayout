use sqlx::sqlite::{SqlitePool, SqliteQueryResult};

use crate::{db::models::TextPreset, utils::errors::ProcessError};

pub async fn select_presets(pool: &SqlitePool, id: i32) -> Result<Vec<TextPreset>, ProcessError> {
    const QUERY: &str = "SELECT * FROM presets WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_all(pool).await?;

    Ok(result)
}

pub async fn update_preset(
    pool: &SqlitePool,
    id: &i32,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str =
        "UPDATE presets SET name = $1, text = $2, x = $3, y = $4, fontsize = $5, line_spacing = $6,
        fontcolor = $7, alpha = $8, box = $9, boxcolor = $10, boxborderw = $11 WHERE id = $12";

    let result = sqlx::query(QUERY)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.x)
        .bind(preset.y)
        .bind(preset.fontsize)
        .bind(preset.line_spacing)
        .bind(preset.fontcolor)
        .bind(preset.alpha)
        .bind(preset.r#box)
        .bind(preset.boxcolor)
        .bind(preset.boxborderw)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn insert_preset(
    pool: &SqlitePool,
    preset: TextPreset,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str =
        "INSERT INTO presets (channel_id, name, text, x, y, fontsize, line_spacing, fontcolor, alpha, box, boxcolor, boxborderw)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)";

    let result = sqlx::query(QUERY)
        .bind(preset.channel_id)
        .bind(preset.name)
        .bind(preset.text)
        .bind(preset.x)
        .bind(preset.y)
        .bind(preset.fontsize)
        .bind(preset.line_spacing)
        .bind(preset.fontcolor)
        .bind(preset.alpha)
        .bind(preset.r#box)
        .bind(preset.boxcolor)
        .bind(preset.boxborderw)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn new_channel_presets(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "INSERT INTO presets (name, text, x, y, fontsize, line_spacing, fontcolor, box, boxcolor, boxborderw, alpha, channel_id)
        VALUES ('Default', 'Welcome to ffplayout messenger!', '(w-text_w)/2', '(h-text_h)/2', '24', '4', '#ffffff@0xff', '0', '#000000@0x80', '4', '1.0', $1),
        ('Empty Text', '', '0', '0', '24', '4', '#000000', '0', '#000000', '0', '0', $1),
        ('Bottom Text fade in', 'The upcoming event will be delayed by a few minutes.', '(w-text_w)/2', '(h-line_h)*0.9', '24', '4', '#ffffff', '1', '#000000@0x80', '4', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),0,if(lt(t,ld(1)+2),(t-(ld(1)+1))/1,if(lt(t,ld(1)+8),1,if(lt(t,ld(1)+9),(1-(t-(ld(1)+8)))/1,0))))', $1),
        ('Scrolling Text', 'We have a very important announcement to make.', 'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),w+4,w-w/12*mod(t-ld(1),12*(w+tw)/w))', '(h-line_h)*0.9', '24', '4', '#ffffff', '1', '#000000@0x80', '4', '1.0', $1);";

    let result = sqlx::query(QUERY).bind(channel_id).execute(pool).await?;

    Ok(result)
}

pub async fn delete_preset(pool: &SqlitePool, id: &i32) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM presets WHERE id = $1;";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}
