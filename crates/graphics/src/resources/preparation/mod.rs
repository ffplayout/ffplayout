//! Bounded CPU preparation and leases shared by a channel's graphics adapters.
use std::{
    collections::VecDeque,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, OnceLock, PoisonError, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, ensure};

mod budget;
mod request;

pub use budget::{JobPermit, ResourceBudget, ResourceLease};
use request::Completion;
pub use request::{Cancellation, Preparation};

type Job = Box<dyn FnOnce(&Cancellation) + Send>;
struct Queue {
    jobs: VecDeque<Job>,
    stopping: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    cancellation: Cancellation,
    resume_requested: AtomicBool,
}
struct Service {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
    budget: ResourceBudget,
}

#[derive(Clone)]
pub struct PreparationService(Arc<Service>);

impl fmt::Debug for PreparationService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparationService")
            .field("usage", &self.0.budget.usage())
            .finish()
    }
}

impl Default for PreparationService {
    fn default() -> Self {
        Self::new(ResourceBudget::new(256 * 1024 * 1024, 1024, 16))
    }
}

impl PreparationService {
    pub fn new(budget: ResourceBudget) -> Self {
        Self(Arc::new(Service {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    jobs: VecDeque::new(),
                    stopping: false,
                }),
                wake: Condvar::new(),
                cancellation: Cancellation::default(),
                resume_requested: AtomicBool::new(true),
            }),
            worker: Mutex::new(None),
            budget,
        }))
    }

    pub fn budget(&self) -> ResourceBudget {
        self.0.budget.clone()
    }

    /// Call at channel/decoder setup, outside frame processing. Idle services
    /// create no thread until preparation is actually requested.
    pub fn start(&self) -> Result<()> {
        let mut worker = match self.0.worker.try_lock() {
            Ok(worker) => worker,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(anyhow!("graphics worker setup busy")),
        };
        ensure!(
            self.recover_locked(&mut worker),
            "graphics preparation is stopping"
        );
        if worker.is_none() {
            let shared = self.0.shared.clone();
            *worker = Some(
                thread::Builder::new()
                    .name("ffplayout-graphics-prepare".into())
                    .spawn(move || {
                        loop {
                            let job = {
                                let mut queue =
                                    shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
                                while queue.jobs.is_empty() && !queue.stopping {
                                    queue = shared
                                        .wake
                                        .wait(queue)
                                        .unwrap_or_else(PoisonError::into_inner);
                                }
                                if queue.stopping {
                                    break;
                                }

                                queue.jobs.pop_front()
                            };
                            if let Some(job) = job {
                                job(&shared.cancellation);
                            }
                        }
                    })?,
            );
        }

        Ok(())
    }

    /// Never waits for a queue lock or free slot. Requests retain only weak
    /// destinations on the worker so cancelled occurrences cannot publish later.
    pub fn submit<T: Send + Sync + 'static>(
        &self,
        operation: impl FnOnce(&Cancellation, &Cancellation) -> Result<T> + Send + 'static,
    ) -> Result<Preparation<T>> {
        self.start()?;
        let permit = self.0.budget.job()?;
        let completion = Arc::new(Completion {
            cancelled: Cancellation::default(),
            ready: OnceLock::new(),
        });
        let weak = Arc::downgrade(&completion);
        let cancelled = completion.cancelled.clone();
        let mut queue = match self.0.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return Err(anyhow!("graphics preparation queue busy"));
            }
        };
        ensure!(!queue.stopping, "graphics preparation is stopping");
        queue.jobs.push_back(Box::new(move |shutdown| {
            let _permit = permit;
            if cancelled.is_cancelled() || shutdown.is_cancelled() {
                return;
            }
            let result = catch_unwind(AssertUnwindSafe(|| operation(&cancelled, shutdown)))
                .unwrap_or_else(|_| Err(anyhow!("graphics preparation panicked")))
                .map_err(|error| format!("{error:#}"));
            if !cancelled.is_cancelled()
                && !shutdown.is_cancelled()
                && let Some(destination) = weak.upgrade()
            {
                let _ = destination.ready.set(result);
            }
        }));
        drop(queue);
        self.0.shared.wake.notify_one();

        Ok(Preparation(completion))
    }

    /// Reuse the channel service across restarts, only after the old worker
    /// has exited. A timed-out worker keeps its slot and blocks replacement.
    pub fn resume(&self) -> bool {
        self.0
            .shared
            .resume_requested
            .store(true, Ordering::Release);

        self.ready()
    }

    /// Retry a requested restart after the previous worker finishes, without
    /// granting permission to reopen a deliberately stopped service.
    pub fn ready(&self) -> bool {
        let mut worker = match self.0.worker.try_lock() {
            Ok(worker) => worker,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };

        self.recover_locked(&mut worker)
    }

    fn recover_locked(&self, worker: &mut Option<JoinHandle<()>>) -> bool {
        if !self.0.shared.cancellation.is_cancelled() {
            return true;
        }
        if !self.0.shared.resume_requested.load(Ordering::Acquire)
            || worker.as_ref().is_some_and(|worker| !worker.is_finished())
        {
            return false;
        }
        let mut queue = match self.0.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if !self.0.shared.resume_requested.load(Ordering::Acquire) {
            return false;
        }
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
        self.0.shared.cancellation.reset();
        queue.stopping = false;

        true
    }

    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Cancellation retains the worker handle until joined. If local rendering
    /// is still finishing after the deadline, no replacement worker is spawned.
    pub fn shutdown(&self, wait: Duration) -> bool {
        self.0.cancel();

        self.join(wait)
    }

    /// Join an already stopping worker without cancelling a newer resume request.
    pub fn join(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            let mut worker = self.0.worker.lock().unwrap_or_else(PoisonError::into_inner);
            if worker.as_ref().is_none_or(JoinHandle::is_finished) {
                if let Some(worker) = worker.take() {
                    let _ = worker.join();
                }
                return true;
            }
            drop(worker);
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Service {
    fn cancel(&self) {
        self.shared.resume_requested.store(false, Ordering::Release);
        self.shared.cancellation.cancel();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queue.stopping = true;
        queue.jobs.clear();
        drop(queue);
        self.shared.wake.notify_all();
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn submit<T: Send + Sync + 'static>(
        service: &PreparationService,
        operation: impl FnOnce(&Cancellation, &Cancellation) -> Result<T> + Send + 'static,
    ) -> Preparation<T> {
        let operation = Arc::new(Mutex::new(Some(operation)));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let operation = operation.clone();
            if let Ok(request) = service.submit(move |cancelled, shutdown| {
                operation.lock().unwrap().take().unwrap()(cancelled, shutdown)
            }) {
                return request;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }

    fn wait<T>(request: &Preparation<T>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while request.result().is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn native_logo_text_and_scene_pixels_share_one_budget_and_queued_surface_leases() {
        use crate::{
            animation::TextPreparation,
            bitmap::StraightRgbaBitmap,
            ffmpeg::{logo::LogoOverlay, scene::CpuScene, text::TextOverlay},
            style::{LogoStyle, TextStyle},
            surface::ResourceKey,
        };
        use ffmpeg_next::{format::Pixel, frame};

        let budget = ResourceBudget::new(4 * 1024 * 1024, 16, 4);
        let mut frame = frame::Video::new(Pixel::YUVA420P, 8, 4);
        for plane in 0..4 {
            frame.data_mut(plane).fill(128);
        }
        let logo = LogoOverlay::from_yuva(
            &LogoStyle {
                scale: None,
                opacity: 1.0,
                position: "0:0".into(),
            },
            frame,
            128,
            72,
        )
        .unwrap()
        .with_budget(&budget)
        .unwrap();
        let logo_bytes = budget.usage().0;
        let text = TextOverlay::prepare_budgeted(
            &TextStyle::default(),
            "MMM",
            TextPreparation {
                canvas_width: 128,
                canvas_height: 72,
                frames_per_second: 25,
                timing: Default::default(),
            },
            Some(&budget),
        )
        .unwrap()
        .unwrap();
        let queued_text = text.snapshot(0, 0).unwrap();
        let text_bytes = budget.usage().0;
        assert!(text_bytes > logo_bytes);
        let mut scene = CpuScene::new(1, 1024 * 1024).with_budget(budget.clone());
        let lease = scene
            .prepare_bitmap(
                ResourceKey { id: 1, revision: 1 },
                &StraightRgbaBitmap {
                    width: 4,
                    height: 4,
                    pixels: [255; 4].repeat(16),
                },
            )
            .unwrap();
        assert!(budget.usage().0 > text_bytes);
        drop(lease);
        scene.retire();
        assert_eq!(budget.usage().0, text_bytes);
        drop(text);
        assert_eq!(
            budget.usage().0,
            text_bytes,
            "queued text retains its budget lease"
        );
        drop(queued_text);
        assert_eq!(budget.usage().0, logo_bytes);
        let positioned = logo.positioned(None, 64, 36).unwrap();
        drop(logo);
        assert_eq!(
            budget.usage().0,
            logo_bytes,
            "logo repositioning shares its lease"
        );
        drop(positioned);
        assert_eq!(budget.usage(), (0, 0, 0));
    }

    #[test]
    fn idle_services_create_no_worker_and_reuse_one_worker_for_multiple_requests() {
        let service = PreparationService::default();
        assert!(service.0.worker.lock().unwrap().is_none());
        let first = submit(&service, |_, _| Ok(thread::current().id()));
        wait(&first);
        let second = submit(&service, |_, _| Ok(thread::current().id()));
        wait(&second);
        assert_eq!(
            first.result().unwrap().as_ref().unwrap(),
            second.result().unwrap().as_ref().unwrap()
        );
        assert!(service.shutdown(Duration::from_secs(1)));
    }

    #[test]
    fn saturated_jobs_fail_fast_cancelled_work_is_not_run_and_permits_are_released() {
        let service = PreparationService::new(ResourceBudget::new(1024, 4, 2));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let active = submit(&service, move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(1)
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let observed = ran.clone();
        let queued = submit(&service, move |_, _| {
            observed.store(true, Ordering::Release);
            Ok(2)
        });
        assert!(service.submit(|_, _| Ok(3)).is_err());
        drop(queued);
        release_tx.send(()).unwrap();
        wait(&active);
        assert!(service.shutdown(Duration::from_secs(1)));
        assert!(!ran.load(Ordering::Acquire));
        assert_eq!(service.budget().usage().2, 0);
    }

    #[test]
    fn timeout_retains_worker_until_it_finishes_and_resume_never_overlaps_workers() {
        let service = PreparationService::default();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let request = submit(&service, move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(1)
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!service.shutdown(Duration::ZERO));
        assert!(!service.resume());
        assert!(service.submit(|_, _| Ok(2)).is_err());
        release_tx.send(()).unwrap();
        assert!(service.shutdown(Duration::from_secs(2)));
        assert!(
            request.result().is_none(),
            "a cancelled job cannot publish after shutdown"
        );
        assert!(service.resume());
        let next = submit(&service, |_, _| Ok(2));
        wait(&next);
        assert_eq!(next.result().unwrap().as_ref().unwrap(), &2);
        assert!(service.shutdown(Duration::from_secs(1)));
    }

    #[test]
    fn a_requested_resume_recovers_on_submission_after_the_old_worker_exits() {
        let service = PreparationService::default();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let old = submit(&service, move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(1)
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!service.shutdown(Duration::ZERO));
        assert!(!service.resume());
        assert!(service.submit(|_, _| Ok(2)).is_err());
        release_tx.send(()).unwrap();
        // The helper retries admission, never calls resume again.
        let next = submit(&service, |_, _| Ok(2));
        wait(&next);
        assert!(old.result().is_none());
        assert_eq!(next.result().unwrap().as_ref().unwrap(), &2);
        assert!(service.shutdown(Duration::from_secs(1)));
        assert!(!service.ready());
        assert!(service.submit(|_, _| Ok(3)).is_err());
    }

    #[test]
    fn job_panics_do_not_kill_the_preparation_worker() {
        let service = PreparationService::default();
        let failed = submit::<()>(&service, |_, _| panic!("test renderer failure"));
        wait(&failed);
        assert!(failed.result().unwrap().is_err());
        let good = submit(&service, |_, _| Ok(3));
        wait(&good);
        assert!(good.result().unwrap().is_ok());
        assert!(service.shutdown(Duration::from_secs(1)));
    }
}
