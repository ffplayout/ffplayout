//! Thread-safe live source selection, separate from transport and playout state.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::LiveListenerConfig;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct LiveInputStatus {
    pub auto_takeover: bool,
    pub id: i32,
    pub session: String,
    pub ready: bool,
    pub selected: bool,
}

#[derive(Debug)]
struct Input {
    source_session: u64,
    token: String,
    video_at: Option<Instant>,
    timeout: Duration,
}

#[derive(Debug, Default)]
struct State {
    relay_thread: Option<std::thread::Thread>,
    listeners: Option<Vec<LiveListenerConfig>>,
    listeners_changed: bool,
    automatic: BTreeMap<i32, bool>,
    revision: u64,
    requested: Option<(i32, u64)>,
    forwarding: Option<(i32, u64)>,
    inputs: BTreeMap<i32, Input>,
}

#[derive(Clone, Debug, Default)]
pub struct LiveIngestControl(Arc<Mutex<State>>);

impl LiveIngestControl {
    /// Replace the desired listener set. The relay applies it independently
    /// of the playout worker, including while a live source is on air.
    pub fn update_listeners(&self, listeners: Vec<LiveListenerConfig>) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.listeners = Some(listeners);
        state.listeners_changed = true;
        if let Some(relay) = &state.relay_thread {
            relay.unpark();
        }
    }

    pub(crate) fn register_relay(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .relay_thread = Some(std::thread::current());
    }

    pub(crate) fn take_listener_update(&self) -> Option<Vec<LiveListenerConfig>> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.listeners_changed {
            return None;
        }
        state.listeners_changed = false;

        state.listeners.clone()
    }

    pub(crate) fn idle_timeout(&self, id: i32) -> Option<crate::LiveIdleTimeout> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .listeners
            .as_ref()?
            .iter()
            .find(|input| input.id == id)
            .map(|input| input.idle_timeout.clone())
    }

    pub(crate) fn invalidate_selection(&self, id: i32) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.revision = state.revision.wrapping_add(1);
        state.forwarding = None;
        if state.requested.is_some_and(|(listener, _)| listener == id) {
            state.requested = None;
        }
    }

    pub fn set_automatic(&self, id: i32, enabled: bool) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Self::change_policy(&mut state, id, enabled);
    }

    pub fn configure_automatic(&self, policies: impl IntoIterator<Item = (i32, bool)>) {
        let policies: BTreeMap<_, _> = policies.into_iter().collect();
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for (&id, &enabled) in &policies {
            Self::change_policy(&mut state, id, enabled);
        }
        let connected: Vec<_> = state.inputs.keys().copied().collect();
        state
            .automatic
            .retain(|id, _| policies.contains_key(id) || connected.contains(id));
    }

    fn change_policy(state: &mut State, id: i32, enabled: bool) {
        if state.automatic.get(&id).copied().unwrap_or(true) == enabled {
            return;
        }
        // Changing policy alone must not interrupt an already selected source.
        if !enabled
            && state.forwarding.is_none()
            && state.requested.is_none()
            && state.inputs.contains_key(&id)
        {
            state.revision = state.revision.wrapping_add(1);
        }
        if !enabled
            && state.forwarding.is_some_and(|(listener, _)| listener == id)
            && state.requested.is_none()
        {
            state.requested = state.forwarding;
        } else if enabled
            && state.requested == state.forwarding
            && state.requested.is_some_and(|(listener, _)| listener == id)
        {
            state.requested = None;
        }
        state.automatic.insert(id, enabled);
    }

    pub fn automatic(&self, id: i32) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .automatic
            .get(&id)
            .copied()
            .unwrap_or(true)
    }

    pub fn snapshot(&self) -> Vec<LiveInputStatus> {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);

        state
            .inputs
            .iter()
            .map(|(&id, input)| LiveInputStatus {
                auto_takeover: state.automatic.get(&id).copied().unwrap_or(true),
                id,
                session: input.token.clone(),
                ready: input
                    .video_at
                    .is_some_and(|at| at.elapsed() < input.timeout),
                selected: state.requested == Some((id, input.source_session))
                    || (state.automatic.get(&id).copied().unwrap_or(true)
                        && state.forwarding == Some((id, input.source_session))),
            })
            .collect()
    }

    /// Explicit, idempotent commands are bound to the publisher session shown
    /// to the caller. A reconnect cannot accidentally accept an old command.
    pub fn select(&self, id: i32, session: &str, active: bool) -> Result<(), &'static str> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.automatic.get(&id).copied().unwrap_or(true) {
            return Err("manual live selection requires automatic takeover to be disabled");
        }
        let input = state.inputs.get(&id).ok_or("live input is not connected")?;
        if input.token != session {
            return Err("live input session has changed");
        }
        if active
            && !input
                .video_at
                .is_some_and(|at| at.elapsed() < input.timeout)
        {
            return Err("live input has no current video signal");
        }
        let selected = (id, input.source_session);
        let requested = if active {
            Some(selected)
        } else if state.requested == Some(selected) {
            None
        } else {
            // Repeating a stop must never stop a subsequently selected source.
            return Ok(());
        };
        if state.requested != requested {
            state.requested = requested;
            state.revision = state.revision.wrapping_add(1);
        }

        Ok(())
    }

    pub(crate) fn revision(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision
    }

    pub(crate) fn started(&self, id: i32, source_session: u64, timeout: Duration) {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let token = format!("{time:x}-{:x}", NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.inputs.insert(
            id,
            Input {
                source_session,
                token,
                video_at: None,
                timeout,
            },
        );
    }

    pub(crate) fn video(&self, id: i32, source_session: u64, timeout: Duration) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(input) = state
            .inputs
            .get_mut(&id)
            .filter(|input| input.source_session == source_session)
        {
            input.video_at = Some(Instant::now());
            input.timeout = timeout;
        }
    }

    pub(crate) fn allows(&self, id: i32, source_session: u64) -> bool {
        self.selection_revision(id, source_session).is_some()
    }

    pub(crate) fn expire_pending(&self) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((id, session)) = state.requested
            && state.forwarding != Some((id, session))
            && !state.inputs.get(&id).is_some_and(|input| {
                input.source_session == session
                    && input
                        .video_at
                        .is_some_and(|at| at.elapsed() < input.timeout)
            })
        {
            state.requested = None;
            state.revision = state.revision.wrapping_add(1);
        }
    }

    pub(crate) fn selection_revision(&self, id: i32, source_session: u64) -> Option<u64> {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);

        (state.requested == Some((id, source_session))
            || (state.automatic.get(&id).copied().unwrap_or(true) && state.requested.is_none()))
        .then_some(state.revision)
    }

    pub(crate) fn forwarding(&self, source: Option<(i32, u64)>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .forwarding = source;
    }

    pub(crate) fn ended(&self, id: i32, source_session: u64) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state
            .inputs
            .get(&id)
            .is_some_and(|input| input.source_session == source_session)
        {
            state.inputs.remove(&id);
        }
        if state.requested == Some((id, source_session)) {
            state.requested = None;
        }
    }

    pub(crate) fn clear(&self) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.inputs.clear();
        state.forwarding = None;
        state.requested = None;
        if let Some(relay) = &state.relay_thread {
            relay.unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(control: &LiveIngestControl, id: i32, session: u64) -> String {
        control.started(id, session, Duration::from_secs(5));
        assert!(
            !control
                .snapshot()
                .iter()
                .find(|input| input.id == id)
                .unwrap()
                .ready
        );
        control.video(id, session, Duration::from_secs(5));

        control
            .snapshot()
            .into_iter()
            .find(|input| input.id == id)
            .unwrap()
            .session
    }

    #[test]
    fn automatic_and_manual_inputs_can_coexist_without_affecting_another_policy() {
        let control = LiveIngestControl::default();
        control.configure_automatic([(1, false), (2, true)]);
        let manual = ready(&control, 1, 10);
        ready(&control, 2, 20);
        assert!(!control.allows(1, 10));
        assert!(control.allows(2, 20));
        control.forwarding(Some((2, 20)));
        control.set_automatic(1, true);
        control.set_automatic(1, false);
        assert_eq!(
            control.revision(),
            0,
            "changing standby policy must not interrupt the on-air input"
        );
        control.select(1, &manual, true).unwrap();
        assert!(control.allows(1, 10));
        assert!(!control.allows(2, 20));
        control.forwarding(Some((1, 10)));
        control.select(1, &manual, false).unwrap();
        assert!(!control.allows(1, 10));
        assert!(control.allows(2, 20));
        let snapshot = control.snapshot();
        assert!(!snapshot[0].auto_takeover);
        assert!(snapshot[1].auto_takeover);
    }

    #[test]
    fn automatic_is_default_and_policy_changes_preserve_the_on_air_source() {
        let control = LiveIngestControl::default();
        assert!(control.automatic(1));
        let session = ready(&control, 1, 10);
        assert!(control.select(1, &session, true).is_err());
        control.forwarding(Some((1, 10)));
        control.configure_automatic([(1, false), (2, false)]);
        assert!(control.allows(1, 10));
        assert!(!control.allows(2, 20));
        assert_eq!(control.revision(), 0);
        control.configure_automatic([(1, true), (2, true)]);
        assert!(control.allows(2, 20));
    }

    #[test]
    fn explicit_selection_is_idempotent_and_old_stop_cannot_stop_another_source() {
        let control = LiveIngestControl::default();
        control.configure_automatic([(1, false), (2, false)]);
        let first = ready(&control, 1, 10);
        let second = ready(&control, 2, 20);
        assert!(!control.allows(1, 10));
        control.select(1, &first, true).unwrap();
        let revision = control.revision();
        control.select(1, &first, true).unwrap();
        assert_eq!(control.revision(), revision);
        control.select(2, &second, true).unwrap();
        control.select(1, &first, false).unwrap();
        assert!(control.allows(2, 20));
        assert!(!control.allows(1, 10));
        control.select(2, &second, false).unwrap();
        let revision = control.revision();
        control.select(2, &second, false).unwrap();
        assert_eq!(control.revision(), revision);
        assert!(!control.allows(2, 20));
    }

    #[test]
    fn reconnect_rejects_old_tokens_and_requires_another_manual_takeover() {
        let control = LiveIngestControl::default();
        control.configure_automatic([(1, false), (2, false)]);
        let old = ready(&control, 1, 10);
        control.select(1, &old, true).unwrap();
        control.ended(1, 10);
        // Even a restarted reader reusing its numeric session gets a new token.
        let new = ready(&control, 1, 10);
        assert_ne!(old, new);
        assert!(control.select(1, &old, true).is_err());
        assert!(!control.allows(1, 10));
        control.select(1, &new, true).unwrap();
        assert!(control.allows(1, 10));
    }

    #[test]
    fn expired_signals_cannot_be_selected_and_pending_switches_expire() {
        let control = LiveIngestControl::default();
        control.configure_automatic([(1, false), (2, false)]);
        let session = ready(&control, 1, 10);
        control.select(1, &session, true).unwrap();
        control
            .0
            .lock()
            .unwrap()
            .inputs
            .get_mut(&1)
            .unwrap()
            .video_at = Some(Instant::now() - Duration::from_secs(10));
        assert!(!control.snapshot()[0].ready);
        assert!(control.select(1, &session, true).is_err());
        control.expire_pending();
        assert!(!control.allows(1, 10));
        assert!(!control.snapshot()[0].selected);
    }
}
