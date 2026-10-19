//! Private playback keeps publication alive while a relay session exists, plus a grace period.
use std::sync::{Arc, Mutex, PoisonError};

use tokio::{
    sync::Notify,
    task::JoinHandle,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

use crate::{player::controller::ChannelManager, utils::errors::ServiceError};

const GRACE: Duration = Duration::from_secs(180);
const STARTUP: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
struct State {
    viewers: usize,
    until: Option<Instant>,
}

#[derive(Debug)]
pub(crate) struct Demand {
    state: Mutex<State>,
    changed: Notify,
    grace: Duration,
    startup: Duration,
}

impl Default for Demand {
    fn default() -> Self {
        Self::new(GRACE, STARTUP)
    }
}

impl Demand {
    fn new(grace: Duration, startup: Duration) -> Self {
        Self {
            state: Mutex::default(),
            changed: Notify::new(),
            grace,
            startup,
        }
    }

    /// Cover the authorized bootstrap/handshake without keeping abandoned HTTP requests alive forever.
    pub(crate) fn request_start(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let until = Instant::now() + self.startup;
        state.until = Some(state.until.map_or(until, |previous| previous.max(until)));
        drop(state);
        self.changed.notify_one();
    }

    pub(crate) fn connect(self: &Arc<Self>) -> Viewer {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.viewers += 1;
        state.until = None;
        drop(state);
        self.changed.notify_one();

        Viewer(self.clone())
    }

    fn snapshot(&self, now: Instant) -> (bool, Option<Instant>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.viewers > 0 {
            return (true, None);
        }
        if state.until.is_some_and(|until| until <= now) {
            state.until = None;
        }

        (state.until.is_some(), state.until)
    }

    pub(crate) fn active(&self) -> bool {
        self.snapshot(Instant::now()).0
    }
}

pub(crate) struct Viewer(Arc<Demand>);

impl Drop for Viewer {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.viewers -= 1;
        if state.viewers == 0 {
            state.until = Some(Instant::now() + self.0.grace);
        }
        drop(state);
        self.0.changed.notify_one();
    }
}

/// One sleeping async task per running channel; no polling or extra OS thread.
pub(crate) struct Watcher(JoinHandle<()>);

impl Drop for Watcher {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) fn watch(manager: ChannelManager, initially_active: bool) -> Watcher {
    Watcher(tokio::spawn(async move {
        run(
            manager.moq_demand.clone(),
            manager.shutdown.clone(),
            initially_active,
            |_| {
                crate::player::output::refresh_moq_demand(&manager).inspect_err(|error| {
                    log::warn!(channel = manager.id; "MoQ demand update failed: {error}");
                })
            },
        )
        .await;
    }))
}

async fn run(
    demand: Arc<Demand>,
    stopped: CancellationToken,
    mut applied: bool,
    mut apply: impl FnMut(bool) -> Result<(), ServiceError>,
) {
    loop {
        let (active, mut deadline) = demand.snapshot(Instant::now());
        if active != applied {
            match apply(active) {
                Ok(()) => applied = active,
                Err(_) => {
                    let retry = Instant::now() + Duration::from_secs(5);
                    deadline = Some(deadline.map_or(retry, |expiry| expiry.min(retry)));
                }
            }
        }

        tokio::select! {
            _ = stopped.cancelled() => break,
            _ = demand.changed.notified() => {},
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn watcher_starts_stops_and_restarts_without_heartbeats() {
        let demand = Arc::new(Demand::new(
            Duration::from_millis(20),
            Duration::from_secs(1),
        ));
        let stopped = CancellationToken::new();
        let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run(demand.clone(), stopped.clone(), false, move |active| {
            send.send(active).unwrap();
            Ok(())
        }));
        demand.request_start();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), receive.recv())
                .await
                .unwrap(),
            Some(true)
        );
        let first = demand.connect();
        let second = demand.connect();
        drop(first);
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(receive.try_recv().is_err());
        drop(second);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), receive.recv())
                .await
                .unwrap(),
            Some(false)
        );
        let viewer = demand.connect();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), receive.recv())
                .await
                .unwrap(),
            Some(true)
        );
        stopped.cancel();
        task.await.unwrap();
        drop(viewer);
    }

    #[test]
    fn only_the_last_viewer_starts_the_three_minute_grace_period() {
        let demand = Arc::new(Demand::default());
        assert!(!demand.active());
        let first = demand.connect();
        let second = demand.connect();
        drop(first);
        assert_eq!(demand.snapshot(Instant::now()), (true, None));
        drop(second);
        let (active, deadline) = demand.snapshot(Instant::now());
        assert!(active);
        let deadline = deadline.unwrap();
        assert!(deadline.saturating_duration_since(Instant::now()) > Duration::from_secs(179));
        assert!(demand.snapshot(deadline - Duration::from_nanos(1)).0);
        assert_eq!(demand.snapshot(deadline), (false, None));
    }

    #[test]
    fn reconnect_cancels_expiry_and_gets_a_fresh_grace_period() {
        let demand = Arc::new(Demand::default());
        drop(demand.connect());
        let old = demand.snapshot(Instant::now()).1.unwrap();
        let viewer = demand.connect();
        assert_eq!(demand.snapshot(old + GRACE), (true, None));
        drop(viewer);
        assert!(demand.snapshot(Instant::now()).1.unwrap() >= old);
    }

    #[test]
    fn an_abandoned_bootstrap_expires_without_shortening_an_existing_grace_period() {
        let demand = Arc::new(Demand::default());
        demand.request_start();
        let deadline = demand.snapshot(Instant::now()).1.unwrap();
        assert_eq!(demand.snapshot(deadline), (false, None));
        drop(demand.connect());
        let grace = demand.snapshot(Instant::now()).1.unwrap();
        demand.request_start();
        assert_eq!(demand.snapshot(Instant::now()).1, Some(grace));
    }
}
