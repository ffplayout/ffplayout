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

    /// Pings clients every 10 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_ping(this: Arc<Self>) {
        tokio::spawn(Box::pin(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                this.broadcast().await;
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

    pub async fn broadcast(&self) {
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

                    if client
                        .sender
                        .send(Ok(Event::default().data(message)))
                        .await
                        .is_err()
                    {
                        failed_clients.push(client.sender.clone());
                    };
                }
                Endpoint::System => {
                    let config = client.manager.config.read().await.clone();
                    let stat = self.system.stat(&config).await;

                    if client
                        .sender
                        .send(Ok(Event::default().data(stat.to_string())))
                        .await
                        .is_err()
                    {
                        failed_clients.push(client.sender.clone());
                    };
                }
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
