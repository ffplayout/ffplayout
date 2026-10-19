use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, OnceLock, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, bail};

pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Cancellation belongs to one playback generation. No queue or worker from
/// that generation is retained when the next clip/seek starts.
#[derive(Default)]
pub(crate) struct PipelineControl {
    cancelled: AtomicBool,
    error: Mutex<Option<String>>,
    wake: Condvar,
    queues: Mutex<Vec<Weak<dyn CancelWake>>>,
    activity: OnceLock<Arc<PipelineActivity>>,
}

/// A generation closes the race between inspecting several queues and sleeping.
/// Created only for coordinators that need to wait on more than one queue.
#[derive(Default, Debug)]
pub(crate) struct PipelineActivity {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl PipelineActivity {
    pub(crate) fn snapshot(&self) -> u64 {
        *self
            .generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn notify(&self) {
        let mut generation = self
            .generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
    }

    pub(crate) fn wait(&self, observed: u64, timeout: Option<Duration>) {
        let generation = self
            .generation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(timeout) = timeout {
            drop(
                self.changed
                    .wait_timeout_while(generation, timeout, |value| *value == observed)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        } else {
            drop(
                self.changed
                    .wait_while(generation, |value| *value == observed)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        }
    }
}

trait CancelWake: Send + Sync {
    fn wake_cancelled(&self);
}

impl PipelineControl {
    pub(crate) fn activity(&self) -> Arc<PipelineActivity> {
        self.activity.get_or_init(Default::default).clone()
    }

    pub(crate) fn notify_activity(&self) {
        if let Some(activity) = self.activity.get() {
            activity.notify();
        }
    }

    pub(crate) fn cancel(&self) {
        {
            let _error = self.error.lock().unwrap_or_else(PoisonError::into_inner);
            self.cancelled.store(true, Ordering::Release);
            self.wake.notify_all();
        }
        let queues: Vec<_> = self
            .queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for queue in queues {
            queue.wake_cancelled();
        }
        self.notify_activity();
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn fail(&self, error: impl std::fmt::Display) {
        let mut stored = self.error.lock().unwrap_or_else(PoisonError::into_inner);
        if stored.is_none() && !self.cancelled() {
            *stored = Some(error.to_string());
        }
        drop(stored);
        self.cancel();
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.cancelled() {
            let error = self.error.lock().unwrap_or_else(PoisonError::into_inner);
            bail!(
                "{}",
                error.as_deref().unwrap_or("playback pipeline cancelled")
            );
        }

        Ok(())
    }

    pub(crate) fn wait(&self, duration: Duration) -> Result<()> {
        let error = self.error.lock().unwrap_or_else(PoisonError::into_inner);
        if self.cancelled() {
            drop(error);
        } else {
            drop(
                self.wake
                    .wait_timeout(error, duration)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        }

        self.check()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct QueueLimits {
    pub(crate) items: usize,
    pub(crate) bytes: usize,
    pub(crate) duration_us: i64,
}

struct Entry<T> {
    item: T,
    bytes: usize,
    timestamp_us: Option<i64>,
}

struct State<T> {
    entries: VecDeque<Entry<T>>,
    bytes: usize,
    closed: bool,
}

pub(crate) struct PipelineQueue<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
    limits: QueueLimits,
    control: Arc<PipelineControl>,
}

impl<T> PipelineQueue<T> {
    pub(crate) fn new(limits: QueueLimits, control: Arc<PipelineControl>) -> Arc<Self>
    where
        T: Send + 'static,
    {
        let queue = Arc::new(Self {
            state: Mutex::new(State {
                entries: VecDeque::new(),
                bytes: 0,
                closed: false,
            }),
            changed: Condvar::new(),
            limits,
            control: control.clone(),
        });
        let wake: Arc<dyn CancelWake> = queue.clone();
        let mut queues = control
            .queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queues.retain(|queue| queue.strong_count() > 0);
        queues.push(Arc::downgrade(&wake));

        queue
    }

    pub(crate) fn send(&self, item: T, bytes: usize, timestamp_us: Option<i64>) -> Result<()> {
        if bytes > self.limits.bytes {
            bail!(
                "media item ({bytes} bytes) exceeds pipeline queue budget ({})",
                self.limits.bytes
            );
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            self.control.check()?;
            if state.closed {
                bail!("playback queue closed");
            }
            let full = self.full(&state, bytes, timestamp_us);
            if !full {
                break;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.entries.push_back(Entry {
            item,
            bytes,
            timestamp_us,
        });
        state.bytes += bytes;
        self.changed.notify_all();
        self.control.notify_activity();

        Ok(())
    }

    fn full(&self, state: &State<T>, bytes: usize, timestamp_us: Option<i64>) -> bool {
        if state.entries.len() >= self.limits.items
            || state.bytes.saturating_add(bytes) > self.limits.bytes
        {
            return true;
        }

        let range = timestamp_us
            .into_iter()
            .chain(state.entries.iter().filter_map(|entry| entry.timestamp_us));
        let (first, last) = range.fold((i64::MAX, i64::MIN), |(first, last), pts| {
            (first.min(pts), last.max(pts))
        });

        !state.entries.is_empty() && last.saturating_sub(first) > self.limits.duration_us
    }

    /// Lets the input scheduler identify cross-track backpressure before a
    /// demux send sleeps. The send itself still rechecks every queue bound.
    pub(crate) fn would_block(&self, bytes: usize, timestamp_us: Option<i64>) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.full(&state, bytes, timestamp_us)
    }

    pub(crate) fn try_send(
        &self,
        item: T,
        bytes: usize,
        timestamp_us: Option<i64>,
    ) -> Result<Option<T>> {
        self.control.check()?;
        if bytes > self.limits.bytes {
            bail!("media item exceeds pipeline queue budget");
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            bail!("playback queue closed");
        }
        if self.full(&state, bytes, timestamp_us) {
            return Ok(Some(item));
        }
        state.entries.push_back(Entry {
            item,
            bytes,
            timestamp_us,
        });
        state.bytes += bytes;
        self.changed.notify_all();
        self.control.notify_activity();

        Ok(None)
    }

    pub(crate) fn try_recv(&self) -> Result<Option<T>> {
        self.control.check()?;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let item = state.entries.pop_front().map(|entry| {
            state.bytes -= entry.bytes;
            entry.item
        });
        if item.is_some() {
            self.changed.notify_all();
            self.control.notify_activity();
        }

        Ok(item)
    }

    pub(crate) fn recv(&self) -> Result<Option<T>> {
        self.recv_until(None)
    }

    /// Delivery workers also have retry/status deadlines. Data and cancellation
    /// wake them immediately; a timeout only bounds their next status check.
    #[cfg(test)]
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<Option<T>> {
        self.recv_until(Some(Instant::now() + timeout))
    }

    fn recv_until(&self, deadline: Option<Instant>) -> Result<Option<T>> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            self.control.check()?;
            if let Some(entry) = state.entries.pop_front() {
                state.bytes -= entry.bytes;
                self.changed.notify_all();
                self.control.notify_activity();

                return Ok(Some(entry.item));
            }
            if state.closed {
                return Ok(None);
            }
            state = if let Some(deadline) = deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(None);
                }
                self.changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            } else {
                self.changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner)
            };
        }
    }

    /// Drop stale media without losing queued rate/subtitle control messages.
    pub(crate) fn discard_where(&self, mut discard: impl FnMut(&T) -> bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.entries.retain(|entry| !discard(&entry.item));
        state.bytes = state.entries.iter().map(|entry| entry.bytes).sum();
        self.changed.notify_all();
        self.control.notify_activity();
    }

    pub(crate) fn front_timestamp(&self) -> Option<i64> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .front()
            .and_then(|entry| entry.timestamp_us)
    }

    pub(crate) fn closed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed
    }

    pub(crate) fn drained(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed && state.entries.is_empty()
    }

    pub(crate) fn close(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed = true;
        self.changed.notify_all();
        self.control.notify_activity();
    }
}

impl<T: Send> CancelWake for PipelineQueue<T> {
    fn wake_cancelled(&self) {
        // Hold the same lock as the predicate check and Condvar::wait. A
        // cancellation can then never be lost between checking and sleeping.
        let _state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.changed.notify_all();
    }
}

pub(crate) fn run_worker(control: &PipelineControl, operation: impl FnOnce() -> Result<()>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .unwrap_or_else(|_| Err(anyhow!("playback worker panicked")));
    if let Err(error) = result {
        control.fail(format!("{error:#}"));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Barrier, mpsc},
        thread,
    };

    use super::*;

    #[test]
    fn activity_detects_events_between_queue_inspection_and_waiting() {
        let control = Arc::new(PipelineControl::default());
        let activity = control.activity();
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            control.clone(),
        );

        for event in 0..4 {
            let observed = activity.snapshot();
            match event {
                0 => queue.send(1, 1, Some(0)).unwrap(),
                1 => {
                    assert_eq!(queue.try_recv().unwrap(), Some(1));
                }
                2 => queue.close(),
                _ => control.cancel(),
            }
            // Notify before waiting: a plain Condvar notification would be lost.
            let (done, receive) = mpsc::channel();
            let waiter = activity.clone();
            let thread = thread::spawn(move || {
                waiter.wait(observed, None);
                done.send(()).unwrap();
            });
            let result = receive.recv_timeout(Duration::from_secs(1));
            activity.notify();
            thread.join().unwrap();
            assert!(result.is_ok(), "activity event {event} was lost");
        }
    }

    #[test]
    fn cancellation_wakes_a_coordinator_waiting_on_multiple_queues() {
        let control = Arc::new(PipelineControl::default());
        let activity = control.activity();
        let observed = activity.snapshot();
        let (done, receive) = mpsc::channel();
        let waiter = activity.clone();
        let waiting = thread::spawn(move || {
            waiter.wait(observed, None);
            done.send(()).unwrap();
        });
        control.cancel();
        let result = receive.recv_timeout(Duration::from_secs(1));
        activity.notify();
        waiting.join().unwrap();
        assert!(result.is_ok());
        assert!(control.check().is_err());
    }

    #[test]
    fn cancellation_releases_full_senders_and_empty_receivers() {
        let control = Arc::new(PipelineControl::default());
        let full = PipelineQueue::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            control.clone(),
        );
        let empty = PipelineQueue::<u8>::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            control.clone(),
        );
        full.send(1_u8, 8, Some(0)).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let (sender, receiver) = mpsc::channel();
        let workers: Vec<_> = [full.clone(), empty]
            .into_iter()
            .enumerate()
            .map(|(index, queue)| {
                let barrier = barrier.clone();
                let sender = sender.clone();
                thread::spawn(move || {
                    barrier.wait();
                    let failed = if index == 0 {
                        queue.send(2, 1, Some(1)).is_err()
                    } else {
                        queue.recv().is_err()
                    };
                    sender.send(failed).unwrap();
                })
            })
            .collect();
        barrier.wait();
        control.cancel();
        for _ in 0..2 {
            assert!(receiver.recv_timeout(Duration::from_millis(250)).unwrap());
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(control.wait(Duration::ZERO).is_err());
    }

    #[test]
    fn blocking_queue_delivers_ordered_data_and_close_without_timed_polling() {
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            Arc::new(PipelineControl::default()),
        );
        let sending = queue.clone();
        let worker = thread::spawn(move || {
            for value in 0..2_000 {
                sending.send(value, 8, None).unwrap();
            }
            sending.close();
        });
        for expected in 0..2_000 {
            assert_eq!(queue.recv().unwrap(), Some(expected));
        }
        assert_eq!(queue.recv().unwrap(), None);
        worker.join().unwrap();
    }

    #[test]
    fn receive_timeout_does_not_close_the_queue_or_discard_later_data() {
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            Arc::new(PipelineControl::default()),
        );
        assert_eq!(queue.recv_timeout(Duration::from_millis(1)).unwrap(), None);
        queue.send(7, 1, None).unwrap();
        assert_eq!(queue.recv_timeout(Duration::ZERO).unwrap(), Some(7));
        assert!(!queue.closed());
    }

    #[test]
    fn failure_wakes_a_receiver_and_retains_the_original_error() {
        let control = Arc::new(PipelineControl::default());
        let queue = PipelineQueue::<u8>::new(
            QueueLimits {
                items: 1,
                bytes: 8,
                duration_us: 100,
            },
            control.clone(),
        );
        let (sender, receiver) = mpsc::channel();
        let worker =
            thread::spawn(move || sender.send(queue.recv().unwrap_err().to_string()).unwrap());
        control.fail("decoder failed");
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            "decoder failed"
        );
        worker.join().unwrap();
    }

    #[test]
    fn queue_bounds_payload_and_media_span_without_losing_rejected_items() {
        let control = Arc::new(PipelineControl::default());
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 4,
                bytes: 8,
                duration_us: 100,
            },
            control,
        );
        queue.send(1, 6, Some(0)).unwrap();
        assert_eq!(queue.try_send(2, 3, Some(1)).unwrap(), Some(2));
        assert_eq!(queue.try_send(3, 1, Some(101)).unwrap(), Some(3));
        assert_eq!(queue.try_send(4, 1, Some(100)).unwrap(), None);
        queue.close();
        assert_eq!(queue.recv().unwrap(), Some(1));
        assert_eq!(queue.recv().unwrap(), Some(4));
        assert_eq!(queue.recv().unwrap(), None);
    }
}
