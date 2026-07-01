use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeDelta, TimeZone};
use log::*;
use protect_axum::authorities::AuthDetails;
use regex::Regex;

use crate::{
    api::{
        routes::{AuthUser, ProgramItem, ProgramObj, ensure_any_authority},
        state::AppState,
    },
    db::models::Role,
    player::utils::{get_date_range, sec_to_time, time_to_sec},
    utils::{errors::ServiceError, playlist::read_playlist},
    vec_strings,
};

/// **Program info**
///
/// Get program infos about given date, or current day
///
/// Examples:
///
/// * get program from current day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get a program range between two dates
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1?start_after=2022-11-13T12:00:00&start_before=2022-11-20T11:59:59 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get program from give day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1?start_after=2022-11-13T10:00:00 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_program(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(obj): Query<ProgramObj>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<ProgramItem>>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await.clone();
    let id = config.general.channel_id;
    let start_sec = config.playlist.start_sec.unwrap();
    let mut days = 0;
    let mut program = vec![];
    let after = obj.start_after;
    let mut before = obj.start_before;

    if after > before {
        before = chrono::Local
            .with_ymd_and_hms(after.year(), after.month(), after.day(), 23, 59, 59)
            .unwrap()
            .naive_local();
    }

    if start_sec
        > time_to_sec(
            &after.format("%H:%M:%S").to_string(),
            &config.channel.timezone,
        )
    {
        days = 1;
    }

    let date_range = get_date_range(
        id,
        &vec_strings![
            (after - TimeDelta::try_days(days).unwrap_or_default()).format("%Y-%m-%d"),
            "-",
            before.format("%Y-%m-%d")
        ],
    );

    for date in date_range {
        let mut naive = NaiveDateTime::parse_from_str(
            &format!("{date} {}", sec_to_time(start_sec)),
            "%Y-%m-%d %H:%M:%S%.3f",
        )
        .unwrap();

        let playlist = match read_playlist(&config, date.clone()).await {
            Ok(p) => p,
            Err(e) => {
                error!("Error in Playlist from {date}: {e}");
                continue;
            }
        };

        for item in playlist.program {
            let start: DateTime<Local> = Local.from_local_datetime(&naive).unwrap();

            let source = match Regex::new(&config.text.regex)
                .ok()
                .and_then(|r| r.captures(&item.source))
            {
                Some(t) => t[1].to_string(),
                None => item.source,
            };

            let p_item = ProgramItem {
                source,
                start: start.format("%Y-%m-%d %H:%M:%S%.3f%:z").to_string(),
                title: item.title,
                r#in: item.seek,
                out: item.out,
                duration: item.duration,
                category: item.category,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive += TimeDelta::try_milliseconds(((item.out - item.seek) * 1000.0) as i64)
                .unwrap_or_default();
        }
    }

    Ok(Json(program))
}
