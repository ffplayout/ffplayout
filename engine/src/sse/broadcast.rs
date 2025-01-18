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
    sync::{mpsc, Mutex},
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
            let mut counter = 0;

            loop {
                interval.tick().await;

                if counter % 10 == 0 {
                    this.remove_stale_clients().await;
                }

                this.broadcast().await;

                counter = (counter + 1) % 61;
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn remove_stale_clients(&self) {
        let mut inner = self.inner.lock().await;

        inner.clients.retain(|client| {
            client
                .sender
                .try_send(sse::Event::Comment("ping".into()))
                .is_ok()
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
        let clients = self.inner.lock().await.clients.clone();
        let mut playout_stat = None;
        let mut system_stat = None;

        if let Some(client) = clients
            .iter()
            .find(|c| matches!(c.endpoint, Endpoint::Playout))
        {
            let media_map = get_data_map(&client.manager).await;
            playout_stat = if client.manager.is_alive.load(Ordering::SeqCst) {
                serde_json::to_string(&media_map).ok()
            } else {
                Some("not running".to_string())
            };
        }

        if let Some(client) = clients
            .iter()
            .find(|c| matches!(c.endpoint, Endpoint::System))
        {
            let config = client.manager.config.lock().await.clone();
            if let Ok(s) = web::block(move || system::stat(&config)).await {
                system_stat = Some(s.to_string());
            }
        }

        for client in clients {
            match client.endpoint {
                Endpoint::Playout => {
                    if let Some(ref pl) = playout_stat {
                        let _ = client.sender.send(sse::Data::new(pl.clone()).into()).await;
                    }
                }
                Endpoint::System => {
                    if let Some(ref sy) = system_stat {
                        let _ = client.sender.send(sse::Data::new(sy.clone()).into()).await;
                    }
                }
            }
        }
    }
}
