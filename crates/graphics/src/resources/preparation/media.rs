//! Tracked media tasks: cancellation is nonblocking, joining belongs to channel shutdown.
use std::{
    sync::PoisonError,
    thread::{self, JoinHandle, Thread},
};

use anyhow::{Result, ensure};

use super::{Cancellation, PreparationService};

pub struct MediaTask {
    stop: Cancellation,
    thread: Thread,
}

impl MediaTask {
    pub fn wake(&self) {
        self.thread.unpark();
    }

    pub fn cancel(&self) {
        self.stop.cancel();
        self.thread.unpark();
    }
}

impl Drop for MediaTask {
    fn drop(&mut self) {
        self.stop.cancel();
        self.thread.unpark();
    }
}

pub(super) struct MediaWorker {
    pub stop: Cancellation,
    pub handle: JoinHandle<()>,
}

impl PreparationService {
    /// Preparation-side only. The task owns its decoder; the service retains
    /// the join handle even after the scene's cancellation ticket is dropped.
    pub fn spawn_media(
        &self,
        operation: impl FnOnce(Cancellation) + Send + 'static,
    ) -> Result<MediaTask> {
        ensure!(self.ready(), "graphics preparation is stopping");
        let mut workers = self.0.media.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(
            !self.0.shared.cancellation.is_cancelled(),
            "graphics preparation is stopping"
        );
        let mut index = 0;
        while index < workers.len() {
            if workers[index].handle.is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.handle.join();
            } else {
                index += 1;
            }
        }
        let permit = self.budget().job()?;
        let stop = Cancellation::default();
        let cancellation = stop.clone();
        let handle = thread::Builder::new()
            .name("ffplayout-graphics-media".into())
            .spawn(move || {
                let _permit = permit;
                operation(cancellation);
            })?;
        let thread = handle.thread().clone();
        workers.push(MediaWorker {
            stop: stop.clone(),
            handle,
        });

        Ok(MediaTask { stop, thread })
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;

    #[test]
    fn cancelled_media_tasks_remain_joinable_and_prevent_overlapping_restart() {
        let service = PreparationService::default();
        let (entered, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let task = service
            .spawn_media(move |stop| {
                entered.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(stop.is_cancelled());
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(task); // Cancellation does not join on the caller/video thread.
        assert!(!service.shutdown(Duration::ZERO));
        assert!(!service.resume());
        assert!(
            service
                .spawn_media(|_| panic!("old task is still running"))
                .is_err()
        );
        release.send(()).unwrap();
        assert!(service.join(Duration::from_secs(2)));
        assert!(service.ready());
        let task = service.spawn_media(|_| {}).unwrap();
        drop(task);
        assert!(service.shutdown(Duration::from_secs(2)));
        assert_eq!(service.budget().usage().2, 0);
    }
}
