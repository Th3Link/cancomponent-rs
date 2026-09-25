use cancomponents_core::relais::State;
use embassy_time::{Duration, Instant};
use heapless::{Entry, FnvIndexMap};

const ZERO: Duration = Duration::from_millis(0);

#[derive(Clone, Debug)]
pub struct ActiveRelais {
    pub current: State,
    pub scheduled: Option<(Instant, State)>,
}

impl ActiveRelais {
    pub fn update(&mut self, now: Instant, new_state: State, duration: embassy_time::Duration) {
        self.current = new_state;
        if duration != ZERO {
            self.scheduled = Some((now + duration, State::Off));
        } else {
            self.scheduled = None;
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<State> {
        if let Some((when, action)) = self.scheduled.clone() {
            if now >= when {
                self.scheduled = None;
                return Some(action);
            }
        }
        None
    }
}

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

    pub fn next_timeout(&self, now: Instant) -> Duration {
        self.relays
            .values()
            .filter_map(|r| {
                r.scheduled
                    .clone()
                    .map(|(t, _)| t.saturating_duration_since(now))
            })
            .min()
            .unwrap_or(Duration::from_millis(100))
    }

    pub fn apply_command(
        &mut self,
        num: usize,
        state: &State,
        duration: embassy_time::Duration,
        now: Instant,
    ) -> bool {
        let changed;

        match self.relays.entry(num) {
            Entry::Occupied(mut entry) => {
                let relay = entry.get_mut();
                changed = &relay.current != state || duration != ZERO;
                relay.update(now, state.clone(), duration);
            }
            Entry::Vacant(entry) => {
                let mut relay = ActiveRelais {
                    current: State::Off,
                    scheduled: None,
                };
                relay.update(now, state.clone(), duration);
                if entry.insert(relay).is_err() {
                    return false;
                }
                changed = true;
            }
        }

        changed
    }

    pub fn poll_expired(&mut self, now: Instant) -> heapless::Vec<(usize, State), N> {
        let mut result = heapless::Vec::new();
        for (&num, relay) in self.relays.iter_mut() {
            if let Some(state) = relay.poll(now) {
                result.push((num, state)).ok(); // ignore overflow
            }
        }
        result
    }
}
