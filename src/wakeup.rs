//! System wakeup scheduler for scheduled tasks when the system is in Sleep.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// A registered wakeup request for the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeupRequest {
    /// Requester tag or identifier (e.g. "daily_schedule", "timer", "sync").
    pub id: String,
    /// Target wake-up time.
    pub at: SystemTime,
    /// Whether the screen should be powered on when woken up by this request (defaults to true).
    pub wake_screen: bool,
}

/// Thread-safe wakeup scheduler.
///
/// Any application, subsystem, or background task can schedule a future wake-up
/// when the system is in low-power Sleep mode.
#[derive(Debug, Default)]
pub struct WakeupScheduler {
    requests: Mutex<BTreeMap<String, WakeupRequest>>,
}

impl WakeupScheduler {
    /// Creates a new, empty wakeup scheduler.
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(BTreeMap::new()),
        }
    }

    /// Schedules a wakeup request at a specific `SystemTime`.
    ///
    /// If a request with `id` already exists, it is replaced.
    pub fn schedule(&self, id: impl Into<String>, at: SystemTime, wake_screen: bool) {
        let id = id.into();
        let mut reqs = self.requests.lock().unwrap();
        reqs.insert(
            id.clone(),
            WakeupRequest {
                id,
                at,
                wake_screen,
            },
        );
    }

    /// Schedules a wakeup request after a given `Duration` from now.
    pub fn schedule_after(&self, id: impl Into<String>, delay: Duration, wake_screen: bool) {
        let at = SystemTime::now() + delay;
        self.schedule(id, at, wake_screen);
    }

    /// Cancels a scheduled wakeup by its `id`.
    /// Returns `true` if a request was found and cancelled.
    pub fn cancel(&self, id: &str) -> bool {
        self.requests.lock().unwrap().remove(id).is_some()
    }

    /// Clears all scheduled wakeup requests.
    pub fn clear(&self) {
        self.requests.lock().unwrap().clear();
    }

    /// Returns the nearest upcoming wakeup request, if any.
    pub fn next_wakeup(&self) -> Option<WakeupRequest> {
        let reqs = self.requests.lock().unwrap();
        reqs.values().min_by_key(|req| req.at).cloned()
    }

    /// Calculates the duration remaining until the nearest scheduled wakeup.
    ///
    /// If the nearest wakeup is already in the past, returns `Some(Duration::ZERO)`.
    /// If no wakeups are scheduled, returns `None`.
    pub fn duration_until_next_wakeup(&self) -> Option<Duration> {
        let next = self.next_wakeup()?;
        let now = SystemTime::now();
        Some(next.at.duration_since(now).unwrap_or(Duration::ZERO))
    }

    /// Polls and consumes all expired wakeups (whose `at <= SystemTime::now()`).
    /// Returns the list of expired wakeup requests.
    pub fn poll_expired(&self) -> Vec<WakeupRequest> {
        let now = SystemTime::now();
        let mut reqs = self.requests.lock().unwrap();
        let mut expired = Vec::new();
        let mut keys_to_remove = Vec::new();

        for (id, req) in reqs.iter() {
            if req.at <= now {
                expired.push(req.clone());
                keys_to_remove.push(id.clone());
            }
        }

        for key in keys_to_remove {
            reqs.remove(&key);
        }

        expired
    }

    /// Returns all currently scheduled requests.
    pub fn all_requests(&self) -> Vec<WakeupRequest> {
        self.requests.lock().unwrap().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_poll_expired() {
        let scheduler = WakeupScheduler::new();
        assert_eq!(scheduler.next_wakeup(), None);
        assert_eq!(scheduler.duration_until_next_wakeup(), None);

        let past = SystemTime::now() - Duration::from_secs(1);
        scheduler.schedule("past_alarm", past, true);

        let future = SystemTime::now() + Duration::from_secs(60);
        scheduler.schedule("future_task", future, false);

        assert_eq!(scheduler.all_requests().len(), 2);
        assert_eq!(scheduler.duration_until_next_wakeup(), Some(Duration::ZERO));

        let expired = scheduler.poll_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "past_alarm");
        assert!(expired[0].wake_screen);

        assert_eq!(scheduler.all_requests().len(), 1);
        assert!(scheduler.cancel("future_task"));
        assert_eq!(scheduler.all_requests().len(), 0);
    }
}
