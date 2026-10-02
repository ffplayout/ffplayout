use chrono::{DateTime, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use chrono_tz::Tz;
use log::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    player::utils::{get_date_range, sec_to_time, time_to_sec},
    utils::{
        config::PlayoutConfig, errors::ServiceError, optional_naive_date_time_from_str,
        playlist::read_playlist, time_machine::time_now,
    },
    vec_strings,
};

#[derive(Debug, Deserialize, Clone)]
pub struct ProgramObj {
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_after: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_before: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize)]
pub struct ProgramItem {
    source: String,
    start: String,
    title: Option<String>,
    r#in: f64,
    out: f64,
    duration: f64,
    ad: bool,
}

pub async fn build_program(
    config: &PlayoutConfig,
    obj: ProgramObj,
) -> Result<Vec<ProgramItem>, ServiceError> {
    let id = config.general.channel_id;
    let start_sec = config.playlist.start_sec.ok_or_else(|| {
        ServiceError::Conflict("Playlist start time has not been initialized".to_string())
    })?;
    let mut days = 0;
    let mut program = vec![];
    let now = time_now(&config.channel.timezone);
    let timezone = now.timezone();
    let today = now.date_naive();
    let after = obj
        .start_after
        .unwrap_or_else(|| today.and_time(NaiveTime::MIN));
    let end_of_day = NaiveTime::from_hms_opt(23, 59, 59)
        .ok_or_else(|| ServiceError::Conflict("Invalid end-of-day time".to_string()))?;
    let mut before = obj
        .start_before
        .unwrap_or_else(|| today.and_time(end_of_day));

    if after > before {
        before = after.date().and_time(end_of_day);
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
    let filename_regex = config
        .text
        .preset
        .as_ref()
        .filter(|preset| preset.use_filename)
        .and_then(|preset| Regex::new(&preset.filename_regex).ok());

    for date in date_range {
        let mut naive = NaiveDateTime::parse_from_str(
            &format!("{date} {}", sec_to_time(start_sec)),
            "%Y-%m-%d %H:%M:%S%.3f",
        )?;

        let playlist = match read_playlist(config, date.clone()).await {
            Ok(p) => p,
            Err(e) => {
                error!("Error in Playlist from {date}: {e}");
                continue;
            }
        };

        for item in playlist.program {
            let start = channel_datetime(timezone, naive)?;

            let source = match filename_regex
                .as_ref()
                .and_then(|regex| regex.captures(&item.source))
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
                ad: item.ad,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive += TimeDelta::try_milliseconds(((item.out - item.seek) * 1000.0) as i64)
                .unwrap_or_default();
        }
    }

    Ok(program)
}

fn channel_datetime(timezone: Tz, naive: NaiveDateTime) -> Result<DateTime<Tz>, ServiceError> {
    timezone
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| {
            ServiceError::Conflict(format!(
                "Local time {naive} does not exist in timezone {timezone}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn program_uses_playlist_timing_channel_timezone_and_date_filter() {
        let root = std::env::temp_dir().join(format!("ffplayout-program-{}", uuid::Uuid::new_v4()));
        let directory = root.join("2026/07");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let playlist = serde_json::json!({
            "date": "2026-07-19",
            "program": [
                {"source": "/media/first.mp4", "title": "First", "in": 10.0, "out": 70.0, "duration": 90.0},
                {"source": "/media/second.mp4", "title": "Second", "in": 0.0, "out": 30.0, "duration": 30.0, "ad": true}
            ]
        });
        tokio::fs::write(directory.join("2026-07-19.json"), playlist.to_string())
            .await
            .unwrap();
        let mut config = PlayoutConfig::default();
        config.channel.playlists = root.clone();
        config.channel.timezone = Some(Tz::America__New_York);
        config.playlist.start_sec = Some(43_200.0);
        let query = ProgramObj {
            start_after: Some(
                NaiveDateTime::parse_from_str("2026-07-19 12:00:00", "%F %T").unwrap(),
            ),
            start_before: Some(
                NaiveDateTime::parse_from_str("2026-07-19 12:00:30", "%F %T").unwrap(),
            ),
        };
        let program = build_program(&config, query).await.unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();

        assert_eq!(program.len(), 1);
        assert_eq!(program[0].source, "/media/first.mp4");
        assert_eq!(program[0].start, "2026-07-19 12:00:00.000-04:00");
        assert_eq!(program[0].r#in, 10.0);
        assert_eq!(program[0].out, 70.0);
        assert_eq!(program[0].duration, 90.0);
    }

    #[tokio::test]
    async fn program_rejects_an_uninitialized_start_time() {
        let config = PlayoutConfig::default();
        let query = ProgramObj {
            start_after: None,
            start_before: None,
        };

        assert!(matches!(
            build_program(&config, query).await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[test]
    fn channel_datetime_uses_channel_timezone() {
        let naive = NaiveDateTime::parse_from_str("2026-07-19 12:00:00", "%F %T").unwrap();

        let date_time = channel_datetime(Tz::America__New_York, naive).unwrap();

        assert_eq!(date_time.format("%:z").to_string(), "-04:00");
    }

    #[test]
    fn channel_datetime_rejects_nonexistent_dst_time() {
        let naive = NaiveDateTime::parse_from_str("2026-03-29 02:30:00", "%F %T").unwrap();

        assert!(channel_datetime(Tz::Europe__Berlin, naive).is_err());
    }
}
