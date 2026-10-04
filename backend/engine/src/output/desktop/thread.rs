//! Main-thread host for desktop output.
//!
//! The application runtime stays on a background thread. Until the first
//! desktop job arrives, the main thread only waits on a channel: headless
//! output does not initialize Winit. Once desktop output is requested, Winit
//! owns the main loop and wakes directly for redraws and queued window jobs.

#[cfg(feature = "tokio")]
use std::sync::{
    Mutex, OnceLock, PoisonError,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
#[cfg(feature = "tokio")]
use std::thread;

#[cfg(feature = "tokio")]
use anyhow::Context;
#[cfg(feature = "tokio")]
use anyhow::{Result, anyhow};
#[cfg(feature = "tokio")]
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
#[cfg(feature = "tokio")]
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
    window::WindowId,
};

#[cfg(feature = "tokio")]
type Job = Box<dyn FnOnce(&ActiveEventLoop) + Send>;

#[cfg(feature = "tokio")]
enum HostMessage {
    Job(Job),
    RuntimeFinished,
}

#[cfg(feature = "tokio")]
struct MainThreadHost {
    sender: mpsc::Sender<HostMessage>,
    proxy: Mutex<Option<EventLoopProxy<()>>>,
    running: AtomicBool,
}

#[cfg(feature = "tokio")]
impl MainThreadHost {
    fn send(&self, message: HostMessage) -> Result<()> {
        if !self.running.load(Ordering::Acquire) {
            return Err(anyhow!("desktop main-thread host stopped"));
        }

        self.sender
            .send(message)
            .map_err(|_| anyhow!("desktop main-thread host stopped"))?;

        if let Some(proxy) = self
            .proxy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            proxy
                .send_event(())
                .map_err(|_| anyhow!("desktop event loop stopped"))?;
        }

        Ok(())
    }
}

#[cfg(feature = "tokio")]
static DESKTOP_MAIN_THREAD: OnceLock<MainThreadHost> = OnceLock::new();

#[cfg(feature = "tokio")]
struct RuntimeCompletion<'a>(&'a MainThreadHost);

#[cfg(feature = "tokio")]
impl Drop for RuntimeCompletion<'_> {
    fn drop(&mut self) {
        // Also wake the main thread if the background runtime panics.
        let _ = self.0.send(HostMessage::RuntimeFinished);
    }
}

/// Runs application work on a background thread and starts Winit on this
/// (main) thread only when desktop output requests a window. Must be called
/// from `main` before a desktop `AsyncPlayout` is opened.
#[cfg(feature = "tokio")]
pub fn run_on_main_thread<R: Send + 'static>(
    background: impl FnOnce() -> R + Send + 'static,
) -> Result<R> {
    let (jobs_tx, jobs_rx) = mpsc::channel();
    DESKTOP_MAIN_THREAD
        .set(MainThreadHost {
            sender: jobs_tx,
            proxy: Mutex::new(None),
            running: AtomicBool::new(true),
        })
        .map_err(|_| anyhow!("desktop main-thread host was initialized more than once"))?;
    let host = DESKTOP_MAIN_THREAD
        .get()
        .ok_or_else(|| anyhow!("desktop main-thread host was not initialized"))?;

    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let runtime = thread::Builder::new()
        .name("ffplayout-runtime".to_string())
        .spawn(move || {
            let _completion = RuntimeCompletion(host);
            let _ = done_tx.send(background());
        })
        .context("failed to start ffplayout runtime thread");

    let result = runtime.and_then(|_| run_host(jobs_rx, host));
    host.running.store(false, Ordering::Release);
    host.proxy
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    super::release_desktop_window();
    result?;

    done_rx
        .recv()
        .context("ffplayout runtime stopped without a result")
}

#[cfg(feature = "tokio")]
fn run_host(receiver: mpsc::Receiver<HostMessage>, host: &MainThreadHost) -> Result<()> {
    let first = receiver
        .recv()
        .context("desktop main-thread host disconnected")?;

    if matches!(first, HostMessage::RuntimeFinished) {
        return Ok(());
    }

    let event_loop = EventLoop::new().context("creating desktop window event loop")?;
    *host.proxy.lock().unwrap_or_else(PoisonError::into_inner) = Some(event_loop.create_proxy());
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = DesktopHostApp {
        receiver,
        first: Some(first),
        resumed: false,
    };

    event_loop
        .run_app(&mut app)
        .context("running desktop event loop")
}

#[cfg(feature = "tokio")]
struct DesktopHostApp {
    receiver: mpsc::Receiver<HostMessage>,
    first: Option<HostMessage>,
    resumed: bool,
}

#[cfg(feature = "tokio")]
impl DesktopHostApp {
    fn drain_jobs(&mut self, event_loop: &ActiveEventLoop) {
        if !self.resumed {
            return;
        }

        loop {
            let message = self.first.take().or_else(|| self.receiver.try_recv().ok());

            match message {
                Some(HostMessage::Job(job)) => job(event_loop),
                Some(HostMessage::RuntimeFinished) => {
                    event_loop.exit();
                    return;
                }
                None => return,
            }
        }
    }
}

#[cfg(feature = "tokio")]
impl ApplicationHandler for DesktopHostApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = true;
        self.drain_jobs(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        self.drain_jobs(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        super::dispatch_desktop_window_event(event_loop, window_id, event);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Native window/GPU resources are released while Winit still has
        // its handler installed, rather than after the event loop returns.
        super::release_desktop_window();
    }
}

/// Schedules a desktop job for execution by the process main thread.
#[cfg(feature = "tokio")]
pub(crate) fn spawn(job: impl FnOnce(&ActiveEventLoop) + Send + 'static) -> Result<()> {
    DESKTOP_MAIN_THREAD
        .get()
        .ok_or_else(|| anyhow!("desktop main-thread host is not running"))?
        .send(HostMessage::Job(Box::new(job)))
}

#[cfg(feature = "tokio")]
pub(crate) fn call<T: Send + 'static>(
    operation: impl FnOnce(&ActiveEventLoop) -> T + Send + 'static,
) -> Result<T> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    spawn(move |event_loop| {
        let _ = result_tx.send(operation(event_loop));
    })?;

    result_rx
        .recv()
        .map_err(|_| anyhow!("desktop main-thread operation stopped"))
}

#[cfg(feature = "tokio")]
pub(crate) fn is_running() -> bool {
    DESKTOP_MAIN_THREAD
        .get()
        .is_some_and(|host| host.running.load(Ordering::Acquire))
}

#[cfg(not(feature = "tokio"))]
pub(crate) fn is_running() -> bool {
    false
}

#[cfg(all(test, feature = "tokio"))]
mod tests {
    use super::*;

    fn host_channel() -> (MainThreadHost, mpsc::Receiver<HostMessage>) {
        let (sender, receiver) = mpsc::channel();

        (
            MainThreadHost {
                sender,
                proxy: Mutex::new(None),
                running: AtomicBool::new(true),
            },
            receiver,
        )
    }

    #[test]
    fn headless_completion_does_not_initialize_winit() {
        let (host, receiver) = host_channel();
        host.send(HostMessage::RuntimeFinished).unwrap();

        run_host(receiver, &host).unwrap();

        assert!(host.proxy.lock().unwrap().is_none());
    }

    #[test]
    fn runtime_panic_notifies_the_waiting_host() {
        let (host, receiver) = host_channel();
        thread::scope(|scope| {
            let runtime = scope.spawn(|| {
                let _completion = RuntimeCompletion(&host);
                panic!("simulated background runtime failure");
            });
            assert!(runtime.join().is_err());
        });

        assert!(matches!(
            receiver.try_recv().unwrap(),
            HostMessage::RuntimeFinished
        ));
        assert!(host.proxy.lock().unwrap().is_none());
    }

    #[test]
    fn stopped_host_rejects_jobs_and_releases_waiting_callers() {
        let (host, _receiver) = host_channel();
        let (result_tx, result_rx) = mpsc::channel::<()>();
        host.running.store(false, Ordering::Release);
        let result = host.send(HostMessage::Job(Box::new(move |_| {
            let _ = result_tx.send(());
        })));

        assert!(result.is_err());
        assert!(matches!(
            result_rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
