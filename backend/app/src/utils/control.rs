use std::{fmt, str::FromStr, sync::atomic::Ordering};

use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{Pool, Sqlite};

use crate::{
    db::{handles, models::TextPreset},
    player::{
        controller::ChannelManager,
        utils::{get_delta, get_media_map},
    },
    utils::{errors::ServiceError, text::text_config},
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ControlParams {
    pub control: PlayerCtl,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessCtl {
    #[default]
    Status,
    Start,
    Stop,
    Restart,
}

impl FromStr for ProcessCtl {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "status" => Ok(Self::Status),
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            _ => Err(format!("Command '{input}' not found!")),
        }
    }
}

impl fmt::Display for ProcessCtl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Status => write!(f, "status"),
            Self::Start => write!(f, "start"),
            Self::Stop => write!(f, "stop"),
            Self::Restart => write!(f, "restart"),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerCtl {
    Back,
    Next,
    #[default]
    Reset,
}

impl FromStr for PlayerCtl {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "back" => Ok(Self::Back),
            "next" => Ok(Self::Next),
            "reset" => Ok(Self::Reset),
            _ => Err(format!("Command '{input}' not found!")),
        }
    }
}

impl fmt::Display for PlayerCtl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Back => write!(f, "back"),
            Self::Next => write!(f, "next"),
            Self::Reset => write!(f, "reset"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Process {
    pub command: ProcessCtl,
}

pub async fn send_message(
    manager: ChannelManager,
    message: TextPreset,
) -> Result<Map<String, Value>, ServiceError> {
    let mut data_map = Map::new();

    let text = (!message.text.trim().is_empty()).then(|| message.text.clone());
    if text.is_none() && !message.use_filename {
        manager.text_overlay.clear();
        data_map.insert("message".to_string(), json!("text overlay cleared"));
        return Ok(data_map);
    }

    manager
        .text_overlay
        .set(Some(text_config(&message, text, message.use_filename)));
    data_map.insert("message".to_string(), json!("text overlay updated"));
    Ok(data_map)
}

pub async fn control_state(
    conn: &Pool<Sqlite>,
    manager: &ChannelManager,
    command: &PlayerCtl,
) -> Result<Map<String, Value>, ServiceError> {
    // Keep the asynchronous manager lock until navigation is committed: a new
    // engine must not replace this control while playlist/DB updates are pending.
    // The engine's reservation also excludes live activation within this run.
    let playback_control = manager.playback_control.lock().await;
    let navigation = match playback_control.begin_navigation() {
        Ok(navigation) => navigation,
        Err(ff_engine::NavigationBlocked::Live) => {
            return Ok(Map::from_iter([
                ("operation".to_string(), json!("ignored")),
                ("reason".to_string(), json!("live_ingest_active")),
            ]));
        }
        Err(ff_engine::NavigationBlocked::Busy) => {
            return Err(ServiceError::Conflict(
                "A navigation command is already being processed".to_string(),
            ));
        }
    };
    let config = manager.config.read().await.clone();
    let id = config.general.channel_id;
    let current_date = manager.current_date.lock().await.clone();
    let current_list = manager.current_list.lock().await.clone();
    let index = manager.current_index.load(Ordering::SeqCst);
    let mut data_map = Map::new();
    let mut shift = 0.0;
    let mut previous_index = None;

    match command {
        PlayerCtl::Back => {
            if index > 1 && current_list.len() > 1 {
                let media = current_list[index - 2].clone();
                (shift, _) = get_delta(&config, &media.begin.unwrap_or(0.0));

                info!(channel = id; "Move to last clip");

                previous_index = Some(index - 2);

                data_map.insert("operation".to_string(), json!("move_to_last"));
                data_map.insert("shifted_seconds".to_string(), json!(shift));
                data_map.insert("media".to_string(), get_media_map(media));
            }
        }

        PlayerCtl::Next => {
            if index < current_list.len() {
                let media = current_list[index].clone();
                (shift, _) = get_delta(&config, &media.begin.unwrap_or(0.0));

                info!(channel = id; "Move to next clip");

                data_map.insert("operation".to_string(), json!("move_to_next"));
                data_map.insert("shifted_seconds".to_string(), json!(shift));
                data_map.insert("media".to_string(), get_media_map(media));
            }
        }

        PlayerCtl::Reset => {
            info!(channel = id; "Reset playout to original state");

            data_map.insert("operation".to_string(), json!("reset_playout_state"));
        }
    }

    handles::update_stat(conn, id, &Some(current_date), shift).await?;
    manager.channel.lock().await.time_shift = shift;
    if let Some(index) = previous_index {
        manager.current_index.store(index, Ordering::SeqCst);
    }
    if *command == PlayerCtl::Reset {
        manager.list_init.store(true, Ordering::SeqCst);
    }
    navigation.commit();
    drop(playback_control);

    Ok(data_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::models::Channel,
        player::utils::{Media, get_data_map},
        utils::{config::PlayoutConfig, system::SystemStat},
    };

    async fn manager() -> ChannelManager {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE channels (id INTEGER PRIMARY KEY, last_date TEXT, time_shift REAL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO channels VALUES (1, '2026-09-08', 42.0)")
            .execute(&pool)
            .await
            .unwrap();
        // Any attempted write is a failure, even if values happen to match.
        sqlx::query("CREATE TRIGGER reject_stat_update BEFORE UPDATE ON channels BEGIN SELECT RAISE(ABORT, 'unexpected status write'); END")
            .execute(&pool).await.unwrap();
        let mut config = PlayoutConfig::default();
        config.general.channel_id = 1;
        config.playlist.start_sec = Some(0.0);
        config.channel.storage = std::env::temp_dir();
        let manager = ChannelManager::new(
            pool,
            Channel {
                id: 1,
                time_shift: 42.0,
                ..Channel::default()
            },
            config,
            tokio_util::sync::CancellationToken::new(),
            SystemStat::default(),
        )
        .await
        .unwrap();
        manager.current_index.store(2, Ordering::SeqCst);
        manager.list_init.store(false, Ordering::SeqCst);
        *manager.current_date.lock().await = "2026-09-08".into();
        *manager.current_list.lock().await = vec![Media::default(); 3];
        manager
    }

    #[tokio::test]
    async fn engine_control_replacement_waits_for_navigation_commit() {
        use std::{
            future::{Future, poll_fn},
            task::Poll,
        };
        let manager = manager().await;
        sqlx::query("DROP TRIGGER reject_stat_update")
            .execute(&manager.db_pool)
            .await
            .unwrap();
        // Pause navigation after it has reserved the current control but before
        // reading configuration or writing any state.
        let config = manager.config.write().await;
        let mut navigation = Box::pin(control_state(&manager.db_pool, &manager, &PlayerCtl::Reset));
        poll_fn(|cx| {
            assert!(navigation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let mut replacement = Box::pin(manager.playback_control.lock());
        poll_fn(|cx| {
            assert!(
                replacement.as_mut().poll(cx).is_pending(),
                "new engine must wait for navigation"
            );
            Poll::Ready(())
        })
        .await;
        drop(config);
        assert_eq!(
            navigation.await.unwrap()["operation"],
            "reset_playout_state"
        );
        let mut current = replacement.await;
        assert_eq!(manager.channel.lock().await.time_shift, 0.0);
        *current = ff_engine::PlaybackControl::default();
        let live = current.try_activate_live().unwrap();
        drop(current);
        let response = control_state(&manager.db_pool, &manager, &PlayerCtl::Back)
            .await
            .unwrap();
        assert_eq!(response["reason"], "live_ingest_active");
        drop(live);
    }

    #[tokio::test]
    async fn cancelled_navigation_releases_engine_replacement_lock() {
        use std::{
            future::{Future, poll_fn},
            task::Poll,
        };
        let manager = manager().await;
        let config = manager.config.write().await;
        let mut navigation = Box::pin(control_state(&manager.db_pool, &manager, &PlayerCtl::Reset));
        poll_fn(|cx| {
            assert!(navigation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(navigation);
        drop(config);
        let current = manager.playback_control.try_lock().unwrap();
        assert!(current.try_activate_live().is_some());
        assert_eq!(manager.channel.lock().await.time_shift, 42.0);
    }

    #[tokio::test]
    async fn live_navigation_is_ignored_without_memory_or_database_changes() {
        let manager = manager().await;
        let control = manager.playback_control.lock().await.clone();
        let live = control.try_activate_live().unwrap();
        for command in [PlayerCtl::Back, PlayerCtl::Next, PlayerCtl::Reset] {
            let response = control_state(&manager.db_pool, &manager, &command)
                .await
                .unwrap();
            assert_eq!(response["operation"], "ignored");
            assert_eq!(response["reason"], "live_ingest_active");
            assert_eq!(manager.current_index.load(Ordering::SeqCst), 2);
            assert!(!manager.list_init.load(Ordering::SeqCst));
            assert_eq!(manager.channel.lock().await.time_shift, 42.0);
        }
        assert_eq!(get_data_map(&manager).await["ingest"], true);
        drop(live);
        assert_eq!(get_data_map(&manager).await["ingest"], false);
        assert!(
            control.try_activate_live().is_some(),
            "ignored commands must not queue navigation"
        );
    }

    #[tokio::test]
    async fn failed_navigation_write_preserves_memory_and_releases_reservation() {
        let manager = manager().await;
        let control = manager.playback_control.lock().await.clone();
        for command in [PlayerCtl::Back, PlayerCtl::Next, PlayerCtl::Reset] {
            assert!(
                control_state(&manager.db_pool, &manager, &command)
                    .await
                    .is_err()
            );
            assert_eq!(manager.current_index.load(Ordering::SeqCst), 2);
            assert!(!manager.list_init.load(Ordering::SeqCst));
            assert_eq!(manager.channel.lock().await.time_shift, 42.0);
            assert!(control.try_activate_live().is_some());
        }
    }

    #[tokio::test]
    async fn navigation_without_live_still_persists_and_requests_a_clip_change() {
        let manager = manager().await;
        sqlx::query("DROP TRIGGER reject_stat_update")
            .execute(&manager.db_pool)
            .await
            .unwrap();
        let response = control_state(&manager.db_pool, &manager, &PlayerCtl::Reset)
            .await
            .unwrap();
        assert_eq!(response["operation"], "reset_playout_state");
        assert!(manager.list_init.load(Ordering::SeqCst));
        assert_eq!(manager.channel.lock().await.time_shift, 0.0);
        let shift: f64 = sqlx::query_scalar("SELECT time_shift FROM channels WHERE id = 1")
            .fetch_one(&manager.db_pool)
            .await
            .unwrap();
        assert_eq!(shift, 0.0);
        assert!(
            manager
                .playback_control
                .lock()
                .await
                .try_activate_live()
                .is_none(),
            "pending navigation must precede a live takeover"
        );
    }
}
