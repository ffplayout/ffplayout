use std::{sync::Arc, time::Duration};

use actix_web::{rt::time::interval, web};
use actix_web_lab::{
    sse::{self, Sse},
    util::InfallibleStream,
};

use ffplayout_lib::utils::PlayoutConfig;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::utils::{control::media_info, system};

#[derive(Debug, Clone)]
struct Client {
    _channel: i32,
    config: PlayoutConfig,
    endpoint: String,
    sender: mpsc::Sender<sse::Event>,
}

impl Client {
    fn new(
        _channel: i32,
        config: PlayoutConfig,
        endpoint: String,
        sender: mpsc::Sender<sse::Event>,
    ) -> Self {
        Self {
            _channel,
            config,
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
        let this = Arc::new(Broadcaster {
            inner: Mutex::new(BroadcasterInner::default()),
        });

        Broadcaster::spawn_ping(Arc::clone(&this));

        this
    }

    /// Pings clients every 10 seconds to see if they are alive and remove them from the broadcast
    /// list if not.
    fn spawn_ping(this: Arc<Self>) {
        actix_web::rt::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            let mut counter = 0;

            loop {
                interval.tick().await;

                if counter % 10 == 0 {
                    this.remove_stale_clients().await;
                }

                if counter % 5 == 0 {
                    this.broadcast_playout().await;
                }

                this.broadcast_system().await;

                counter = (counter + 1) % 61;
            }
        });
    }

    /// Removes all non-responsive clients from broadcast list.
    async fn remove_stale_clients(&self) {
        let clients = self.inner.lock().clients.clone();

        let mut ok_clients = Vec::new();

        for client in clients {
            if client
                .sender
                .send(sse::Event::Comment("ping".into()))
                .await
                .is_ok()
            {
                ok_clients.push(client.clone());
            }
        }

        self.inner.lock().clients = ok_clients;
    }

    /// Registers client with broadcaster, returning an SSE response body.
    pub async fn new_client(
        &self,
        channel: i32,
        config: PlayoutConfig,
        endpoint: String,
    ) -> Sse<InfallibleStream<ReceiverStream<sse::Event>>> {
        let (tx, rx) = mpsc::channel(10);

        tx.send(sse::Data::new("connected").into()).await.unwrap();

        self.inner
            .lock()
            .clients
            .push(Client::new(channel, config, endpoint, tx));

        Sse::from_infallible_receiver(rx)
    }

    /// Broadcasts playout status to clients.
    pub async fn broadcast_playout(&self) {
        let clients = self.inner.lock().clients.clone();

        for client in clients.iter().filter(|client| client.endpoint == "playout") {
            match media_info(&client.config, "current".into()).await {
                Ok(res) => {
                    let _ = client
                        .sender
                        .send(
                            sse::Data::new(res.text().await.unwrap_or_else(|_| "Success".into()))
                                .into(),
                        )
                        .await;
                }
                Err(_) => {
                    let _ = client
                        .sender
                        .send(sse::Data::new("not running").into())
                        .await;
                }
            };
        }
    }

    /// Broadcasts system status to clients.
    pub async fn broadcast_system(&self) {
        let clients = self.inner.lock().clients.clone();

        for client in clients {
            if &client.endpoint == "system" {
                if let Ok(stat) = web::block(move || system::stat(client.config.clone())).await {
                    let stat_string = stat.to_string();
                    let _ = client.sender.send(sse::Data::new(stat_string).into()).await;
                };
            }
        }
    }
}
