//! Scheduling logic for timed relay/rollershutter commands (e.g. "turn on
//! for 500ms"), decoupled from any hardware I/O.
//!
//! [`RelayManager`] never reads the clock itself — every method takes `now`
//! as a parameter — so it's fully deterministic and host-testable. The
//! caller (in `cc-hardware`) is responsible for calling [`RelayManager::poll_expired`]
//! at (or after) the moment [`RelayManager::next_timeout`] indicates, and
//! for actually driving the hardware relay when a state changes.

use crate::relais::State;
use embassy_time::{Duration, Instant};
use heapless::{Entry, FnvIndexMap};

const ZERO: Duration = Duration::from_millis(0);

#[derive(Clone, Debug)]
pub struct ActiveRelais {
    pub current: State,
    /// When set, the time at which `current` should automatically revert
    /// to the given state (used for e.g. "on for 500ms" commands).
    pub scheduled: Option<(Instant, State)>,
}

impl ActiveRelais {
    /// Applies a new commanded state. If `duration` is non-zero, schedules
    /// an automatic revert to `Off` after `duration`; otherwise clears any
    /// pending schedule.
    pub fn update(&mut self, now: Instant, new_state: State, duration: Duration) {
        self.current = new_state;
        if duration != ZERO {
            self.scheduled = Some((now + duration, State::Off));
        } else {
            self.scheduled = None;
        }
    }

    /// Returns the scheduled state if its time has arrived, clearing the
    /// schedule so it only fires once.
    pub fn poll(&mut self, now: Instant) -> Option<State> {
        if let Some((when, action)) = self.scheduled {
            if now >= when {
                self.scheduled = None;
                return Some(action);
            }
        }
        None
    }
}

/// Tracks up to `N` relay/rollershutter channels and their pending
/// auto-revert schedules.
pub struct RelayManager<const N: usize> {
    relays: FnvIndexMap<usize, ActiveRelais, N>,
}

impl<const N: usize> Default for RelayManager<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> RelayManager<N> {
    pub fn new() -> Self {
        Self {
            relays: FnvIndexMap::new(),
        }
    }

    /// How long the caller should wait before calling [`Self::poll_expired`]
    /// again: the time until the earliest pending schedule, or a 100ms
    /// fallback poll interval if nothing is scheduled.
    pub fn next_timeout(&self, now: Instant) -> Duration {
        self.relays
            .values()
            .filter_map(|r| r.scheduled.map(|(t, _)| t.saturating_duration_since(now)))
            .min()
            .unwrap_or(Duration::from_millis(100))
    }

    /// Applies an incoming command to channel `num`. Returns `true` if the
    /// channel's state (or its schedule) actually changed, i.e. the caller
    /// should drive the hardware and report the new state.
    pub fn apply_command(
        &mut self,
        num: usize,
        state: &State,
        duration: Duration,
        now: Instant,
    ) -> bool {
        let changed;

        match self.relays.entry(num) {
            Entry::Occupied(mut entry) => {
                let relay = entry.get_mut();
                changed = &relay.current != state || duration != ZERO;
                relay.update(now, *state, duration);
            }
            Entry::Vacant(entry) => {
                let mut relay = ActiveRelais {
                    current: State::Off,
                    scheduled: None,
                };
                relay.update(now, *state, duration);
                if entry.insert(relay).is_err() {
                    // Table full: drop the command rather than panic.
                    return false;
                }
                changed = true;
            }
        }

        changed
    }

    /// Reverts every channel whose schedule has expired by `now`. The
    /// caller should drive the hardware for each returned `(num, state)`
    /// pair and report it, mirroring what happens for a live command.
    pub fn poll_expired(&mut self, now: Instant) -> heapless::Vec<(usize, State), N> {
        let mut result = heapless::Vec::new();
        for (&num, relay) in self.relays.iter_mut() {
            if let Some(state) = relay.poll(now) {
                result.push((num, state)).ok(); // table capacity bounds this; ignore overflow
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        Instant::from_millis(ms)
    }

    #[test]
    fn apply_command_reports_change_on_new_channel() {
        let mut mgr: RelayManager<4> = RelayManager::new();
        let changed = mgr.apply_command(0, &State::On, ZERO, t(0));
        assert!(changed);
    }

    #[test]
    fn apply_command_is_a_no_op_when_state_and_duration_unchanged() {
        let mut mgr: RelayManager<4> = RelayManager::new();
        mgr.apply_command(0, &State::On, ZERO, t(0));
        let changed_again = mgr.apply_command(0, &State::On, ZERO, t(10));
        assert!(!changed_again);
    }

    #[test]
    fn timed_command_schedules_auto_off_and_expires() {
        let mut mgr: RelayManager<4> = RelayManager::new();
        mgr.apply_command(0, &State::On, Duration::from_millis(500), t(0));

        // Not yet due.
        assert!(mgr.poll_expired(t(400)).is_empty());

        // Due: reverts to Off exactly once.
        let expired = mgr.poll_expired(t(500));
        assert_eq!(expired.as_slice(), &[(0, State::Off)]);
        assert!(mgr.poll_expired(t(600)).is_empty());
    }

    #[test]
    fn next_timeout_falls_back_to_100ms_when_nothing_scheduled() {
        let mgr: RelayManager<4> = RelayManager::new();
        assert_eq!(mgr.next_timeout(t(0)), Duration::from_millis(100));
    }

    #[test]
    fn next_timeout_tracks_the_earliest_schedule() {
        let mut mgr: RelayManager<4> = RelayManager::new();
        mgr.apply_command(0, &State::On, Duration::from_millis(500), t(0));
        mgr.apply_command(1, &State::On, Duration::from_millis(100), t(0));
        assert_eq!(mgr.next_timeout(t(0)), Duration::from_millis(100));
    }

    #[test]
    fn zero_duration_command_clears_a_previous_schedule() {
        let mut mgr: RelayManager<4> = RelayManager::new();
        mgr.apply_command(0, &State::On, Duration::from_millis(500), t(0));
        mgr.apply_command(0, &State::Off, ZERO, t(10));
        assert!(mgr.poll_expired(t(1000)).is_empty());
    }
}
