use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use actix_web::web;
use actix_web_lab::{
    sse::{self, Sse},
    util::InfallibleStream,
};
use tokio::{
    sync::{
        mpsc::{self, error::SendError},
        Mutex,
    },
    time::interval,
};
use tokio_stream::wrappers::ReceiverStream;

use crate::player::{controller::ChannelManager, utils::get_data_map};
use crate::sse::Endpoint;
use crate::utils::system;

#[derive(Debug, Clone)]
struct Client {
    manager: ChannelManager,
    endpoint: Endpoint,
    sender: mpsc::Sender<sse::Event>,
}

impl Client {
    fn new(manager: ChannelManager, endpoint: Endpoint, sender: mpsc::Sender<sse::Event>) -> Self {
        Self {
            manager,
            endpoint,
            sender,
        }
    }
}

pub struct Broadcaster {
    inner: Mutex<BroadcasterInner>,
}

#[derive(Debug, Clone, Default)]
struct BroadcasterInner {
    clients: Vec<Client>,
}

impl Broadcaster {
    /// Constructs new broadcaster and spawns ping loop.
    pub fn create() -> Arc<Self> {
        let this = Arc::new(Self {
            inner: Mutex::new(BroadcasterInner::default()),
        });

        Self::spawn_ping(Arc::clone(&this));

        this
    }

    /// Pings clients every 10 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_ping(this: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                this.broadcast().await;
            }
        });
    }

    /// Registers client with broadcaster, returning an SSE response body.
    pub async fn new_client(
        &self,
        manager: ChannelManager,
        endpoint: Endpoint,
    ) -> Sse<InfallibleStream<ReceiverStream<sse::Event>>> {
        let (tx, rx) = mpsc::channel(10);

        tx.send(sse::Data::new("connected").into()).await.unwrap();

        let client = Client::new(manager, endpoint, tx);
        self.inner.lock().await.clients.push(client);

        Sse::from_infallible_receiver(rx)
    }

    pub async fn broadcast(&self) {
        let mut inner = self.inner.lock().await;
        let mut failed_clients = Vec::new();

        // every client needs its own stats
        for (index, client) in inner.clients.iter().enumerate() {
            let mut sender_result = Err(SendError(sse::Event::Comment("closed".into())));

            match client.endpoint {
                Endpoint::Playout => {
                    let media_map = get_data_map(&client.manager).await;

                    let message = if client.manager.is_alive.load(Ordering::SeqCst) {
                        serde_json::to_string(&media_map).unwrap_or_default()
                    } else {
                        "not running".to_string()
                    };

                    sender_result = client.sender.send(sse::Data::new(message).into()).await;
                }
                Endpoint::System => {
                    let config = client.manager.config.lock().await.clone();

                    if let Ok(stat) = web::block(move || system::stat(&config)).await {
                        sender_result = client
                            .sender
                            .send(sse::Data::new(stat.to_string()).into())
                            .await;
                    }
                }
            }

            if sender_result.is_err() {
                failed_clients.push(index);
            }
        }

        for &index in failed_clients.iter().rev() {
            inner.clients.remove(index);
        }
    }
}
