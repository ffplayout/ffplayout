use jiff::{
    SignedDuration, ToSpan, ZonedDateTime,
    civil::{DateTime, Time},
    tz::{AmbiguousOffset, TimeZone},
};
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

const MAX_PROGRAM_DAYS: i32 = 366;

#[derive(Debug, Deserialize, Clone)]
pub struct ProgramObj {
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_after: Option<DateTime>,
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_before: Option<DateTime>,
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
    let timezone = now.time_zone();
    let today = now.date();
    let after = obj
        .start_after
        .unwrap_or_else(|| today.to_datetime(Time::MIN));
    let end_of_day = Time::constant(23, 59, 59, 0);
    let mut before = obj
        .start_before
        .unwrap_or_else(|| today.to_datetime(end_of_day));

    if after > before {
        before = after.date().to_datetime(end_of_day);
    }

    // Every day of the range is a playlist lookup; keep the request bounded.
    let requested_days = (before.date() - after.date()).get_days();

    if requested_days > MAX_PROGRAM_DAYS {
        return Err(ServiceError::BadRequest(format!(
            "Program range must not exceed {MAX_PROGRAM_DAYS} days"
        )));
    }

    if start_sec
        > time_to_sec(
            &after.strftime("%H:%M:%S").to_string(),
            &config.channel.timezone,
        )
    {
        days = 1;
    }

    let date_range = get_date_range(
        id,
        &vec_strings![
            after.date().checked_sub(days.days())?,
            "-",
            before.strftime("%Y-%m-%d")
        ],
    );
    let filename_regex = config
        .text
        .preset
        .as_ref()
        .filter(|preset| preset.use_filename)
        .and_then(|preset| Regex::new(&preset.filename_regex).ok());

    for date in date_range {
        let mut naive = DateTime::strptime(
            "%Y-%m-%d %H:%M:%S%.3f",
            format!("{date} {}", sec_to_time(start_sec)),
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
                // A pattern without a capture group selects the whole match.
                Some(captures) => captures
                    .get(1)
                    .or_else(|| captures.get(0))
                    .map_or_else(|| item.source.clone(), |found| found.as_str().to_string()),
                None => item.source,
            };

            let p_item = ProgramItem {
                source,
                start: start.strftime("%Y-%m-%d %H:%M:%S%.3f%:z").to_string(),
                title: item.title,
                r#in: item.seek,
                out: item.out,
                duration: item.duration,
                ad: item.ad,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive = naive.checked_add(SignedDuration::from_millis(
                ((item.out - item.seek) * 1000.0) as i64,
            ))?;
        }
    }

    Ok(program)
}

fn channel_datetime(timezone: &TimeZone, naive: DateTime) -> Result<ZonedDateTime, ServiceError> {
    let local = timezone.to_ambiguous_zoned_datetime(naive);

    if matches!(local.offset(), AmbiguousOffset::Gap { .. }) {
        return Err(ServiceError::Conflict(format!(
            "Local time {naive} does not exist in timezone {}",
            timezone.iana_name().unwrap_or("local")
        )));
    }

    Ok(local.earlier()?)
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
        config.channel.timezone = Some(TimeZone::get("America/New_York").unwrap());
        config.playlist.start_sec = Some(43_200.0);
        let query = ProgramObj {
            start_after: Some(DateTime::strptime("%F %T", "2026-07-19 12:00:00").unwrap()),
            start_before: Some(DateTime::strptime("%F %T", "2026-07-19 12:00:30").unwrap()),
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
        let naive = DateTime::strptime("%F %T", "2026-07-19 12:00:00").unwrap();

        let date_time =
            channel_datetime(&TimeZone::get("America/New_York").unwrap(), naive).unwrap();

        assert_eq!(date_time.strftime("%:z").to_string(), "-04:00");
    }

    #[test]
    fn program_bounds_keep_the_wall_time_from_offset_inputs() {
        let expected = DateTime::strptime("%F %T", "2026-07-19 12:00:00").unwrap();

        for input in [
            "2026-07-19T12:00:00",
            "2026-07-19T12:00:00+02:00",
            "2026-07-19T12:00:00+0200",
            "2026-07-19T12:00:00+02",
            "2026-07-19T12:00:00Z",
        ] {
            let query: ProgramObj =
                serde_json::from_value(serde_json::json!({ "start_after": input })).unwrap();

            assert_eq!(query.start_after, Some(expected), "{input}");
        }

        let query: ProgramObj =
            serde_json::from_value(serde_json::json!({ "start_after": "2026-07-19" })).unwrap();

        assert_eq!(
            query.start_after,
            Some(expected.date().to_datetime(Time::MIN))
        );
        assert!(
            serde_json::from_value::<ProgramObj>(
                serde_json::json!({ "start_after": "2026-02-30T12:00:00" })
            )
            .is_err()
        );
    }

    #[test]
    fn channel_datetime_rejects_nonexistent_dst_time() {
        let naive = DateTime::strptime("%F %T", "2026-03-29 02:30:00").unwrap();

        assert!(channel_datetime(&TimeZone::get("Europe/Berlin").unwrap(), naive).is_err());
    }

    #[test]
    fn channel_datetime_uses_the_earlier_occurrence_during_a_dst_fold() {
        let timezone = TimeZone::get("Europe/Berlin").unwrap();
        let naive = DateTime::strptime("%F %T", "2026-10-25 02:30:00").unwrap();
        let zoned = channel_datetime(&timezone, naive).unwrap();

        assert_eq!(
            zoned.strftime("%FT%T%:z").to_string(),
            "2026-10-25T02:30:00+02:00"
        );
    }
}
