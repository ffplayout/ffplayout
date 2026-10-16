use std::{
    env,
    net::SocketAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result};
use hang::moq_net;
use moq_relay::{Config, Relay};
use tokio_util::sync::CancellationToken;

use crate::utils::logging::Target;

use super::access::{self, Tickets};

pub(super) const RETENTION: Duration = Duration::from_secs(2);
pub(super) static RUNTIME: OnceLock<Arc<Runtime>> = OnceLock::new();

pub(super) struct Runtime {
    pub(super) origin: moq_net::origin::Producer,
    pub(super) port: u16,
    pub(super) public_url: Option<String>,
    pub(super) tickets: Arc<Tickets>,
}

pub struct Server {
    stopped: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn start(pool: sqlx::SqlitePool) -> Result<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let bind: SocketAddr = env::var("FFPLAYOUT_MOQ_BIND")
            .unwrap_or_else(|_| "0.0.0.0:8089".into())
            .parse()
            .context("invalid FFPLAYOUT_MOQ_BIND")?;
        let public_url = env::var("FFPLAYOUT_MOQ_PUBLIC_URL").ok();
        let mut config = Config::default();
        config.listen.bind = Some(bind.to_string().parse()?);
        config.listen.tls.generate = vec!["localhost".into(), "127.0.0.1".into()];
        config.web.http.listen = Some(bind);
        config.cache.capacity = Some("32MiB".into());
        config.cache.duration = Some(RETENTION);
        config.drain_timeout = Duration::from_millis(500);
        let mut relay = Relay::load(config).await?;
        let stopped = CancellationToken::new();
        let tickets = Arc::new(Tickets::default());
        let admissions = relay.admissions().context("MoQ admissions unavailable")?;
        access::run(admissions, pool, tickets.clone(), stopped.clone());
        let port = relay
            .web_addrs()
            .http
            .context("MoQ HTTP listener unavailable")?
            .port();
        let runtime = Arc::new(Runtime {
            origin: relay.cluster().origin.clone(),
            port,
            public_url,
            tickets,
        });
        RUNTIME
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("MoQ runtime already initialized"))?;
        let token = stopped.clone();
        let trigger = relay.shutdown_trigger().clone();
        let task = tokio::spawn(async move {
            let run = relay.with_signals(false).run();
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => if let Err(error) = result { log::error!(target: Target::Console.as_str(), "MoQ relay stopped: {error}"); },
                _ = token.cancelled() => { trigger.start(); if let Err(error) = run.await { log::warn!(target: Target::Console.as_str(), "MoQ relay shutdown: {error}"); } }
            }
        });
        log::info!(target: Target::Console.as_str(), "Experimental MoQ relay listening on {bind} (TCP/UDP)");

        Ok(Self { stopped, task })
    }

    pub async fn stop(self) {
        self.stopped.cancel();
        let mut task = self.task;

        if tokio::time::timeout(Duration::from_secs(5), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
}
