use std::{
    mem,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Default)]
struct NavigationState {
    live_active: bool,
    live_listener_id: Option<i32>,
    reserved: bool,
    requested: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackControl {
    skip_current: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    navigation: Arc<Mutex<NavigationState>>,
}

impl PlaybackControl {
    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Unconditionally interrupt playback, including live ingest (e.g. shutdown).
    pub fn skip_current(&self) {
        self.skip_current.store(true, Ordering::SeqCst);
    }

    pub fn restart_playout(&self) {
        self.restart.store(true, Ordering::SeqCst);
    }

    pub fn live_active(&self) -> bool {
        self.live_status().0
    }

    /// Snapshot the live takeover state without observing an ID from a
    /// different session. The listener ID is absent when no live source is on
    /// air or when a caller activates the legacy unlabelled session.
    pub fn live_status(&self) -> (bool, Option<i32>) {
        let state = self
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        (state.live_active, state.live_listener_id)
    }

    /// Reserve playlist navigation before changing playlist or persisted state.
    /// Live activation cannot overtake this reservation. Dropping it without
    /// committing releases it without requesting a skip.
    pub fn begin_navigation(&self) -> Result<PlaylistNavigation, NavigationBlocked> {
        if self.is_shutdown() {
            return Err(NavigationBlocked::Busy);
        }

        let mut state = self
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if state.live_active {
            return Err(NavigationBlocked::Live);
        }

        if state.reserved {
            return Err(NavigationBlocked::Busy);
        }

        state.reserved = true;

        Ok(PlaylistNavigation {
            control: self.clone(),
        })
    }

    /// Mark an actual live takeover. Keep the returned lease for the entire
    /// session, including across playlist calls; dropping it clears the status.
    /// A reserved or not-yet-consumed navigation request must finish first.
    pub fn try_activate_live(&self) -> Option<LiveSession> {
        self.activate_live(None)
    }

    pub fn try_activate_live_for_listener(&self, listener_id: i32) -> Option<LiveSession> {
        self.activate_live(Some(listener_id))
    }

    fn activate_live(&self, listener_id: Option<i32>) -> Option<LiveSession> {
        let mut state = self
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if state.live_active || state.reserved || state.requested {
            return None;
        }

        state.live_active = true;
        state.live_listener_id = listener_id;

        Some(LiveSession {
            control: self.clone(),
        })
    }

    pub(crate) fn take_skip_current(&self) -> bool {
        self.skip_current.swap(false, Ordering::SeqCst)
    }

    pub(crate) fn take_restart(&self) -> bool {
        self.restart.swap(false, Ordering::SeqCst)
    }

    pub(crate) fn take_navigation(&self) -> bool {
        let mut state = self
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.live_active || state.reserved {
            return false;
        }
        mem::take(&mut state.requested)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum NavigationBlocked {
    Live,
    Busy,
}

pub struct PlaylistNavigation {
    control: PlaybackControl,
}

impl PlaylistNavigation {
    pub fn commit(self) {
        let mut state = self
            .control
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.requested = true;
        // Drop releases the reservation after this lock, making the request
        // visible only after the caller has completed all state updates.
    }
}

impl Drop for PlaylistNavigation {
    fn drop(&mut self) {
        self.control
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserved = false;
    }
}

pub struct LiveSession {
    control: PlaybackControl,
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        let mut state = self
            .control
            .navigation
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        state.live_active = false;
        state.live_listener_id = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;

    use super::*;

    #[test]
    fn live_listener_id_is_visible_only_during_its_session() {
        let control = PlaybackControl::default();
        assert_eq!(control.live_status(), (false, None));

        let live = control.try_activate_live_for_listener(42).unwrap();
        assert_eq!(control.live_status(), (true, Some(42)));
        assert!(control.try_activate_live_for_listener(7).is_none());
        assert_eq!(control.live_status(), (true, Some(42)));

        drop(live);
        assert_eq!(control.live_status(), (false, None));

        let unlabelled_live = control.try_activate_live().unwrap();
        assert_eq!(control.live_status(), (true, None));
        drop(unlabelled_live);
        assert_eq!(control.live_status(), (false, None));
    }

    #[test]
    fn shutdown_remains_effective_across_repeated_playback_checks() {
        let control = PlaybackControl::default();
        let _live = control.try_activate_live().unwrap();
        control.request_shutdown();
        assert!(matches!(
            control.begin_navigation(),
            Err(NavigationBlocked::Busy)
        ));

        for _ in 0..3 {
            assert!(
                crate::playout::check_playback_control(&control)
                    .unwrap_err()
                    .is::<crate::playout::PlaybackRestart>()
            );
        }
    }

    #[test]
    fn live_blocks_navigation_but_not_stop_or_restart() {
        let control = PlaybackControl::default();
        let live = control.try_activate_live().unwrap();
        assert!(control.live_active());
        assert!(matches!(
            control.begin_navigation(),
            Err(NavigationBlocked::Live)
        ));
        control.skip_current();
        control.restart_playout();
        assert!(control.take_skip_current());
        assert!(control.take_restart());
        assert!(!control.take_navigation());
        drop(live);
        assert!(!control.live_active());
        assert!(
            !control.take_navigation(),
            "ignored navigation must not be deferred"
        );
        assert!(control.begin_navigation().is_ok());
    }

    #[test]
    fn live_waits_for_navigation_to_commit_and_be_consumed() {
        let control = PlaybackControl::default();
        let navigation = control.begin_navigation().unwrap();
        assert!(control.try_activate_live().is_none());
        assert!(!control.take_navigation());
        navigation.commit();
        assert!(control.try_activate_live().is_none());
        assert!(control.take_navigation());
        assert!(
            !control.take_skip_current(),
            "navigation is a separate signal"
        );
        assert!(control.try_activate_live().is_some());
    }

    #[test]
    fn cancelled_navigation_releases_live_without_skipping() {
        let control = PlaybackControl::default();
        drop(control.begin_navigation().unwrap());
        assert!(!control.take_navigation());
        assert!(control.try_activate_live().is_some());
    }

    #[test]
    fn simultaneous_navigation_and_live_takeover_are_mutually_exclusive() {
        let control = PlaybackControl::default();
        let barrier = Arc::new(Barrier::new(2));
        let worker_control = control.clone();
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            let live = worker_control.try_activate_live();
            worker_barrier.wait();
            live
        });
        barrier.wait();
        let navigation = control.begin_navigation();
        barrier.wait();
        let live = worker.join().unwrap();
        assert_ne!(navigation.is_ok(), live.is_some());
    }
}
