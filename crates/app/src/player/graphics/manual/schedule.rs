use std::{collections::BTreeMap, sync::atomic::Ordering, time::Duration};

use ff_engine::ClipGraphics;
use jiff::{
    SignedDuration, Timestamp, ZonedDateTime,
    civil::{Date, Time},
    tz::{AmbiguousOffset, TimeZone},
};
use log::warn;
use serde_json::{Value, json};
use tokio::{task::JoinHandle, time::Instant};

use crate::{
    db::{
        handles,
        models::{GraphicAppointment, GraphicLayer},
    },
    player::controller::ChannelManager,
    utils::{errors::ServiceError, time_machine::time_now},
};

use super::{has_runtime, preparation, runtime};

const PREPARATION_LEAD_US: i64 = 3_000_000;

#[derive(Clone, Copy)]
struct Window {
    start: Timestamp,
    end: Timestamp,
}

#[derive(Default)]
pub(super) struct Timing {
    active_from: Option<Timestamp>,
    pub(super) active_until: Option<Timestamp>,
    pub(super) next_start: Option<Timestamp>,
}

pub(super) fn timing(appointments: &[GraphicAppointment], now: &ZonedDateTime) -> Timing {
    let mut upcoming = Vec::new();
    let mut timing = Timing::default();
    let current = now.timestamp();

    for appointment in appointments
        .iter()
        .filter(|appointment| appointment.enabled)
    {
        let (previous, next) = occurrences(appointment, now);

        if let Some(window) = previous.filter(|window| window.end > current) {
            timing.active_from = Some(
                timing
                    .active_from
                    .map_or(window.start, |start| start.min(window.start)),
            );
            timing.active_until = timing.active_until.max(Some(window.end));
        }

        if let Some(window) = next {
            upcoming.push(window);
        }
    }

    upcoming.sort_unstable_by_key(|window| window.start);
    timing.next_start = upcoming.first().map(|window| window.start);

    if let Some(end) = &mut timing.active_until {
        for window in upcoming {
            if window.start > *end {
                break;
            }

            *end = (*end).max(window.end);
        }
    }

    timing
}

fn occurrences(
    appointment: &GraphicAppointment,
    now: &ZonedDateTime,
) -> (Option<Window>, Option<Window>) {
    let current = now.timestamp();
    let timezone = now.time_zone();

    if let Some(date) = appointment.date {
        let window = window_on(appointment, timezone, date);

        return (
            window.filter(|window| window.start <= current),
            window.filter(|window| window.start > current),
        );
    }

    let today = now.date();
    let previous = scan(
        appointment,
        timezone,
        appointment.end_date.map_or(today, |end| today.min(end)),
        Date::yesterday,
        |window| window.start <= current,
    );
    let next = scan(
        appointment,
        timezone,
        appointment
            .start_date
            .map_or(today, |start| today.max(start)),
        Date::tomorrow,
        |window| window.start > current,
    );

    (previous, next)
}

fn scan(
    appointment: &GraphicAppointment,
    timezone: &TimeZone,
    start: Date,
    step: impl Fn(Date) -> Result<Date, jiff::Error>,
    matches: impl Fn(&Window) -> bool,
) -> Option<Window> {
    // Allow a weekly occurrence to be skipped during a daylight-saving gap.
    std::iter::successors(Some(start), |date| step(*date).ok())
        .take(15)
        .filter(|date| {
            appointment.start_date.is_none_or(|start| *date >= start)
                && appointment.end_date.is_none_or(|end| *date <= end)
                && appointment
                    .weekdays
                    .contains(&date.weekday().to_monday_one_offset())
        })
        .filter_map(|date| window_on(appointment, timezone, date))
        .find(matches)
}

fn window_on(appointment: &GraphicAppointment, timezone: &TimeZone, date: Date) -> Option<Window> {
    let start = local_timestamp(timezone, date, appointment.start_time)?;
    let end = if let Some(seconds) = appointment.duration_seconds {
        start
            .checked_add(SignedDuration::from_secs(i64::from(seconds)))
            .ok()?
    } else {
        let time = appointment.end_time?;
        let date = if time < appointment.start_time {
            date.tomorrow().ok()?
        } else {
            date
        };

        local_timestamp(timezone, date, time)?
    };

    (end > start).then_some(Window { start, end })
}

fn local_timestamp(timezone: &TimeZone, date: Date, time: Time) -> Option<Timestamp> {
    let local = timezone.to_ambiguous_zoned_datetime(date.to_datetime(time));

    if matches!(local.offset(), AmbiguousOffset::Gap { .. }) {
        return None;
    }

    Some(local.earlier().ok()?.timestamp())
}

struct ScheduledLayer {
    row: GraphicLayer,
    until: Timestamp,
    fade_out_us: i64,
}

fn scheduled_row(row: &GraphicLayer) -> GraphicLayer {
    let mut row = row.clone();
    row.visible = true;

    if let Some(settings) = row.settings.as_object_mut() {
        for key in ["duration", "hold", "pause", "repeat"] {
            settings.remove(key);
        }

        settings.insert("source_end".into(), json!("hold_last"));
    }

    row
}

type PreparedLayer = (GraphicLayer, Result<ClipGraphics, ServiceError>);

struct FailedPreparation {
    settings: Value,
    until: Timestamp,
}

#[derive(Default)]
struct ScheduledLayers {
    active: BTreeMap<i64, ScheduledLayer>,
    failed: BTreeMap<i64, FailedPreparation>,
    ready: BTreeMap<i64, PreparedLayer>,
}

pub(super) async fn run(manager: &ChannelManager, epoch: usize) {
    let state = &manager.manual_state;
    let mut snapshot = None;
    let mut version = 0;
    let mut reported_db_error = false;
    let mut layers = ScheduledLayers::default();
    let mut worker: Option<JoinHandle<PreparedLayer>> = None;

    while runtime::epoch_is_running(manager, epoch) {
        let operation = state.operations.lock().await;

        if !runtime::epoch_is_running(manager, epoch) {
            break;
        }

        let current_version = state.schedule_version.load(Ordering::Acquire);

        if snapshot.is_none() || current_version != version {
            match handles::select_graphics(&manager.db_pool, manager.id).await {
                Ok((_, rows, schedules)) => {
                    snapshot = Some((rows, schedules));
                    version = current_version;
                    reported_db_error = false;
                }
                Err(error) => {
                    snapshot = None;

                    if !reported_db_error {
                        warn!(channel = manager.id; "Could not load graphics schedules: {error}");
                        reported_db_error = true;
                    }
                }
            }
        }

        let period = match &snapshot {
            Some((_, schedules)) if schedules.is_empty() => Duration::from_secs(5),
            _ => Duration::from_secs(1),
        };
        let mut deadline = Instant::now() + period;

        if let Some((rows, schedules)) = &snapshot {
            let timezone = manager.config.read().await.channel.timezone.clone();
            let now = time_now(&timezone);
            let tick = Instant::now();

            let is_scheduled =
                |id: &i64| rows.iter().any(|row| row.layer_id == *id && !row.running);

            layers.active.retain(|id, _| is_scheduled(id));
            layers.failed.retain(|id, _| is_scheduled(id));
            layers.ready.retain(|id, _| is_scheduled(id));

            for row in rows.iter().filter(|row| !row.running) {
                if !schedules.contains_key(&row.layer_id)
                    && !layers.active.contains_key(&row.layer_id)
                    && !layers.failed.contains_key(&row.layer_id)
                    && !layers.ready.contains_key(&row.layer_id)
                {
                    continue;
                }

                let timing = schedules
                    .get(&row.layer_id)
                    .map_or_else(Timing::default, |appointments| timing(appointments, &now));

                let candidate = layers.reconcile(manager, row, &timing, now.timestamp());

                if let Some(delay) =
                    next_wakeup(&timing, now.timestamp(), layers.active.get(&row.layer_id))
                {
                    deadline = deadline.min(tick + delay);
                }

                if let Some(row) = candidate
                    && worker.is_none()
                {
                    let manager = manager.clone();

                    worker = Some(tokio::spawn(async move {
                        let result =
                            preparation::prepare_scheduled(&manager, row.settings.0.clone()).await;

                        (row, result)
                    }));
                }
            }
        }

        drop(operation);

        tokio::select! {
            () = state.schedule_changed.notified() => {},
            () = tokio::time::sleep_until(deadline) => {},
            result = async { worker.as_mut().unwrap().await }, if worker.is_some() => {
                worker = None;

                match result {
                    Ok(prepared) => {
                        layers.ready.insert(prepared.0.layer_id, prepared);
                    }
                    Err(error) => warn!(channel = manager.id; "Scheduled graphics preparation task failed: {error}"),
                }
            }
        }
    }

    if let Some(worker) = worker {
        let _ = worker.await;
    }
}

// Use the actual start, fade and end deadlines. The regular wake-up in run()
// also makes the scheduler recalculate if the channel timezone or wall clock changes.
fn next_wakeup(
    timing: &Timing,
    now: Timestamp,
    active: Option<&ScheduledLayer>,
) -> Option<Duration> {
    let current = now.as_microsecond();
    let deadline = if let Some(end) = timing.active_until {
        let end = end.as_microsecond();
        let fade_start = active
            .filter(|layer| layer.row.visible)
            .map(|layer| end - layer.fade_out_us);

        fade_start.filter(|start| *start > current).unwrap_or(end)
    } else {
        let start = timing.next_start?.as_microsecond();
        let preparation = start - PREPARATION_LEAD_US;

        if preparation > current {
            preparation
        } else {
            start
        }
    };

    let delay = u64::try_from(deadline - current).unwrap_or_default();

    Some(Duration::from_micros(delay.max(1)))
}

impl ScheduledLayers {
    fn reconcile(
        &mut self,
        manager: &ChannelManager,
        row: &GraphicLayer,
        timing: &Timing,
        now: Timestamp,
    ) -> Option<GraphicLayer> {
        let id = row.layer_id;
        let desired = scheduled_row(row);
        let previous = self.active.remove(&id);
        let state = &manager.manual_state;

        if self
            .ready
            .get(&id)
            .is_some_and(|(row, _)| row.settings != desired.settings)
        {
            self.ready.remove(&id);
        }

        let Some(until) = timing.active_until else {
            if let Some(previous) = previous {
                remove(manager, previous);
            }

            if self.failed.remove(&id).is_some() {
                state.clear_error(id);
            }

            if timing.next_start.is_some_and(|start| {
                start.as_microsecond() - now.as_microsecond() <= PREPARATION_LEAD_US
            }) {
                return (!self.ready.contains_key(&id)).then_some(desired);
            }

            self.ready.remove(&id);

            return None;
        };

        let remaining_us = until.as_microsecond() - now.as_microsecond();

        if let Some(mut previous) = previous {
            let continuous = timing
                .active_from
                .is_some_and(|start| start <= previous.until);

            if continuous && state.scheduled(id) && has_runtime(manager, id) {
                set_scheduled_visibility(manager, &mut previous, remaining_us);
                previous.until = until;
                let matches = state.loaded(&desired);

                if matches {
                    previous.row.settings = desired.settings.clone();
                    if self.failed.remove(&id).is_some() {
                        state.clear_error(id);
                    }
                    self.ready.remove(&id);
                }

                self.active.insert(id, previous);

                if matches {
                    return None;
                }
            } else {
                remove(manager, previous);
            }
        }

        if self
            .failed
            .get(&id)
            .is_some_and(|failed| failed.settings == desired.settings.0 && now < failed.until)
        {
            self.ready.remove(&id);

            return None;
        }

        let Some((_, prepared)) = self.ready.remove(&id) else {
            return Some(desired);
        };

        match prepared.and_then(|graphics| runtime::activate(manager, &desired, Some(graphics))) {
            Ok(()) => {
                state.mark_scheduled(id);
                self.failed.remove(&id);

                let fade_out = fade_out_us(&desired);
                let fade_out_us = if fade_out >= remaining_us {
                    remaining_us / 2
                } else {
                    fade_out
                };

                self.active.insert(
                    id,
                    ScheduledLayer {
                        row: desired,
                        until,
                        fade_out_us,
                    },
                );
            }
            Err(error) => {
                self.failed.insert(
                    id,
                    FailedPreparation {
                        settings: desired.settings.0,
                        until,
                    },
                );
                state.set_error(id, &error);
                warn!(channel = manager.id; "Could not activate scheduled graphic layer {id}: {error}");
            }
        }

        None
    }
}

fn fade_out_us(row: &GraphicLayer) -> i64 {
    let seconds = row.settings["fade_out"].as_f64().unwrap_or(0.0);

    (seconds * 1_000_000.0) as i64
}

fn set_scheduled_visibility(
    manager: &ChannelManager,
    previous: &mut ScheduledLayer,
    remaining_us: i64,
) {
    let visible = remaining_us > previous.fade_out_us;

    if previous.row.visible == visible {
        return;
    }

    let mut change = previous.row.clone();
    change.visible = visible;
    change.settings["fade_out"] =
        json!(previous.fade_out_us.min(remaining_us) as f64 / 1_000_000.0);

    if let Err(error) = runtime::change(manager, &change, "visibility") {
        manager.manual_state.set_error(change.layer_id, &error);
    }

    previous.row.visible = visible;
}

fn remove(manager: &ChannelManager, previous: ScheduledLayer) {
    let state = &manager.manual_state;
    let id = previous.row.layer_id;

    if !state.scheduled(id) {
        return;
    }

    if has_runtime(manager, id)
        && let Err(error) = runtime::change(manager, &previous.row, "clear")
    {
        state.set_error(id, &error);
        return;
    }

    state.forget(Some(id));
}

#[cfg(test)]
mod tests {
    use crate::utils::time_machine::parse_channel_timezone;

    use super::*;

    fn at(time: &str, zone: &str) -> ZonedDateTime {
        time.parse::<Timestamp>()
            .unwrap()
            .to_zoned_datetime(parse_channel_timezone(zone).unwrap())
    }

    #[test]
    fn short_exit_fade_uses_its_deadline_between_regular_wakeups() {
        use sqlx::types::Json;

        let end = "2026-11-23T12:00:01Z".parse::<Timestamp>().unwrap();
        let before_fade = "2026-11-23T12:00:00.91Z".parse().unwrap();
        let at_fade = "2026-11-23T12:00:00.92Z".parse().unwrap();
        let timing = Timing {
            active_until: Some(end),
            ..Timing::default()
        };
        let mut active = ScheduledLayer {
            row: GraphicLayer {
                layer_id: 1,
                name: String::new(),
                settings: Json(json!({"fade_out": 0.08})),
                running: false,
                visible: true,
            },
            until: end,
            fade_out_us: 80_000,
        };

        assert_eq!(
            next_wakeup(&timing, before_fade, Some(&active)),
            Some(Duration::from_millis(10))
        );
        active.row.visible = false;

        assert_eq!(
            next_wakeup(&timing, at_fade, Some(&active)),
            Some(Duration::from_millis(80))
        );
    }

    #[test]
    fn overlapping_and_adjacent_appointments_keep_one_continuous_window() {
        let appointments: Vec<GraphicAppointment> = serde_json::from_value(json!([
            {"enabled":true,"date":"2026-11-23","start_time":"12:00","end_time":"12:10"},
            {"enabled":true,"weekdays":[1],"start_time":"12:08","duration_seconds":420},
            {"enabled":true,"date":"2026-11-23","start_time":"12:15","end_time":"12:20"},
            {"enabled":false,"date":"2026-11-23","start_time":"12:15","end_time":"13:00"}
        ]))
        .unwrap();
        let now = at("2026-11-23T11:05:00Z", "Europe/Berlin");
        let current = timing(&appointments, &now);

        assert!(appointments.iter().all(GraphicAppointment::is_valid));
        assert_eq!(
            current.next_start,
            Some("2026-11-23T11:08:00Z".parse().unwrap())
        );
        assert_eq!(
            current.active_until,
            Some("2026-11-23T11:20:00Z".parse().unwrap())
        );
    }

    #[test]
    fn weekly_bounds_apply_to_the_start_date_when_an_appointment_crosses_midnight() {
        let appointments: Vec<GraphicAppointment> = serde_json::from_value(json!([
            {"enabled":true,"weekdays":[1,7],"start_time":"23:55","end_time":"00:10",
             "start_date":"2026-11-23","end_date":"2026-11-23"}
        ]))
        .unwrap();

        let before = timing(&appointments, &at("2026-11-22T22:55:00Z", "Europe/Berlin"));
        let active = timing(&appointments, &at("2026-11-23T23:05:00Z", "Europe/Berlin"));
        let after = timing(&appointments, &at("2026-11-23T23:10:00Z", "Europe/Berlin"));

        assert!(before.active_until.is_none());
        assert_eq!(
            before.next_start,
            Some("2026-11-23T22:55:00Z".parse().unwrap())
        );
        assert_eq!(
            active.active_until,
            Some("2026-11-23T23:10:00Z".parse().unwrap())
        );
        assert!(active.next_start.is_none());
        assert!(after.active_until.is_none());
    }

    #[test]
    fn daylight_saving_skips_missing_times_and_runs_ambiguous_times_once() {
        let appointments: Vec<GraphicAppointment> = serde_json::from_value(json!([
            {"enabled":true,"weekdays":[7],"start_time":"02:30","duration_seconds":3600}
        ]))
        .unwrap();
        let spring = timing(&appointments, &at("2026-03-29T00:55:00Z", "Europe/Berlin"));
        let fall = timing(&appointments, &at("2026-10-25T01:20:00Z", "Europe/Berlin"));
        let after = timing(&appointments, &at("2026-10-25T01:45:00Z", "Europe/Berlin"));

        assert_eq!(
            spring.next_start,
            Some("2026-04-05T00:30:00Z".parse().unwrap())
        );
        assert_eq!(
            fall.active_until,
            Some("2026-10-25T01:30:00Z".parse().unwrap())
        );
        assert!(after.active_until.is_none());
        assert_eq!(
            after.next_start,
            Some("2026-11-01T01:30:00Z".parse().unwrap())
        );
    }
}
