use std::{
    convert::Infallible,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use axum::response::{
    IntoResponse,
    sse::{Event, KeepAlive, Sse},
};
use serde_json::{Map, Value, json};
use tokio::{
    sync::{Mutex, mpsc},
    time::{MissedTickBehavior, interval},
};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    player::{controller::ChannelManager, utils::get_data_map},
    sse::Endpoint,
    utils::system::SystemStat,
};

fn playout_status_message(mut media: Map<String, Value>, running: bool) -> String {
    if running {
        return serde_json::to_string(&media).unwrap_or_default();
    }
    let mut inputs = media.remove("ingest_inputs").unwrap_or_else(|| json!([]));
    if let Some(inputs) = inputs.as_array_mut() {
        for input in inputs {
            input["ready"] = json!(false);
            input["selected"] = json!(false);
            input["session"] = json!("");
        }
    }

    json!({ "running": false, "ingest_inputs": inputs }).to_string()
}

#[derive(Debug, Clone)]
struct Client {
    manager: ChannelManager,
    endpoint: Endpoint,
    audio_meter: bool,
    sender: mpsc::Sender<Result<Event, Infallible>>,
}

impl Client {
    fn new(
        manager: ChannelManager,
        endpoint: Endpoint,
        audio_meter: bool,
        sender: mpsc::Sender<Result<Event, Infallible>>,
    ) -> Self {
        Self {
            manager,
            endpoint,
            audio_meter,
            sender,
        }
    }
}

fn try_send_event(sender: &mpsc::Sender<Result<Event, Infallible>>, event: Event) -> bool {
    sender.try_send(Ok(event)).is_ok()
}

#[derive(Clone)]
pub struct Broadcaster {
    inner: Arc<Mutex<BroadcasterInner>>,
    pub system: SystemStat,
}

#[derive(Debug, Clone, Default)]
struct BroadcasterInner {
    clients: Vec<Client>,
}

const SSE_INTERVAL: Duration = Duration::from_millis(250);
// Keep the system-statistics cadence at one second while playout status is
// broadcast four times per second.
const SYSTEM_UPDATE_INTERVAL_TICKS: u64 = 4;

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn create(system: SystemStat) -> Arc<Self> {
        let this = Arc::new(Self {
            inner: Arc::new(Mutex::new(BroadcasterInner::default())),
            system,
        });

        Self::spawn_ping(Arc::clone(&this));

        this
    }

    /// Broadcasts updates and removes clients that no longer consume them.
    fn spawn_ping(this: Arc<Self>) {
        tokio::spawn(Box::pin(async move {
            let mut interval = interval(SSE_INTERVAL);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut tick = 0_u64;

            loop {
                interval.tick().await;
                tick = tick.wrapping_add(1);

                this.broadcast(tick.is_multiple_of(SYSTEM_UPDATE_INTERVAL_TICKS))
                    .await;
            }
        }));
    }

    /// Registers client with broadcaster, returning an SSE response body.
    pub async fn new_client(
        &self,
        manager: ChannelManager,
        endpoint: Endpoint,
        audio_meter: bool,
    ) -> impl IntoResponse {
        let (tx, rx) = mpsc::channel(10);

        tx.send(Ok(Event::default().data("connected")))
            .await
            .unwrap();

        if audio_meter {
            manager.loudness_meter.subscribe();
        }
        let client = Client::new(manager, endpoint, audio_meter, tx);
        self.inner.lock().await.clients.push(client);

        Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
    }

    pub async fn broadcast(&self, include_system: bool) {
        let clients = {
            let inner = self.inner.lock().await;
            inner.clients.clone()
        };
        let mut failed_clients = Vec::new();

        // every client needs its own stats
        for client in &clients {
            match client.endpoint {
                Endpoint::Playout => {
                    let media_map = get_data_map(&client.manager).await;

                    let message = playout_status_message(
                        media_map,
                        client.manager.is_alive.load(Ordering::SeqCst),
                    );

                    if !try_send_event(&client.sender, Event::default().data(message)) {
                        failed_clients.push(client.sender.clone());
                    };
                }
                Endpoint::System if include_system => {
                    let config = client.manager.config.read().await.clone();
                    let stat = self.system.stat(&config).await;

                    if !try_send_event(&client.sender, Event::default().data(stat.to_string())) {
                        failed_clients.push(client.sender.clone());
                    };
                }
                Endpoint::System => {}
            }
        }

        if failed_clients.is_empty() {
            return;
        }

        let mut inner = self.inner.lock().await;
        inner.clients.retain(|client| {
            let remove = failed_clients
                .iter()
                .any(|failed| failed.same_channel(&client.sender));
            if remove && client.audio_meter {
                client.manager.loudness_meter.unsubscribe();
            }
            !remove
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_playout_still_reports_input_labels_without_a_live_signal() {
        let media = json!({ "audio": { "rms": -5 }, "ingest_inputs": [{
            "id": 3, "name": "Renamed input", "auto_takeover": false,
            "ready": true, "selected": true, "session": "old-session"
        }] })
        .as_object()
        .unwrap()
        .clone();
        let message: Value =
            serde_json::from_str(&playout_status_message(media.clone(), false)).unwrap();
        assert_eq!(message["running"], false);
        assert_eq!(message["ingest_inputs"][0]["name"], "Renamed input");
        assert_eq!(message["ingest_inputs"][0]["ready"], false);
        assert_eq!(message["ingest_inputs"][0]["selected"], false);
        assert_eq!(message["ingest_inputs"][0]["session"], "");
        assert!(message.get("audio").is_none());
        let running: Value =
            serde_json::from_str(&playout_status_message(media.clone(), true)).unwrap();
        assert_eq!(running, json!(media));
    }

    #[test]
    fn full_client_queue_is_rejected_without_waiting() {
        let (sender, _receiver) = mpsc::channel(1);

        assert!(try_send_event(&sender, Event::default().data("first")));
        assert!(!try_send_event(&sender, Event::default().data("second")));
    }

    #[test]
    fn closed_client_queue_is_rejected() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);

        assert!(!try_send_event(&sender, Event::default().data("event")));
    }
}
