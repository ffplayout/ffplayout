use std::{
    convert::Infallible,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use axum::response::{
    IntoResponse,
    sse::{Event, KeepAlive, Sse},
};
use tokio::{
    sync::{Mutex, mpsc},
    time::interval,
};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    player::{controller::ChannelManager, utils::get_data_map},
    sse::Endpoint,
    utils::system::SystemStat,
};

#[derive(Debug, Clone)]
struct Client {
    manager: ChannelManager,
    endpoint: Endpoint,
    sender: mpsc::Sender<Result<Event, Infallible>>,
}

impl Client {
    fn new(
        manager: ChannelManager,
        endpoint: Endpoint,
        sender: mpsc::Sender<Result<Event, Infallible>>,
    ) -> Self {
        Self {
            manager,
            endpoint,
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
            let mut interval = interval(Duration::from_millis(500));
            let mut tick = 0_u64;

            loop {
                interval.tick().await;
                tick = tick.wrapping_add(1);

                this.broadcast(tick.is_multiple_of(2)).await;
            }
        }));
    }

    /// Registers client with broadcaster, returning an SSE response body.
    pub async fn new_client(
        &self,
        manager: ChannelManager,
        endpoint: Endpoint,
    ) -> impl IntoResponse {
        let (tx, rx) = mpsc::channel(10);

        tx.send(Ok(Event::default().data("connected")))
            .await
            .unwrap();

        let client = Client::new(manager, endpoint, tx);
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

                    let message = if client.manager.is_alive.load(Ordering::SeqCst) {
                        serde_json::to_string(&media_map).unwrap_or_default()
                    } else {
                        "not running".to_string()
                    };

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
            !failed_clients
                .iter()
                .any(|failed| failed.same_channel(&client.sender))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
