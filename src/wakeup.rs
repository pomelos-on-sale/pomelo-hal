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

pub type WakeupChangeListener = Box<dyn Fn(Option<SystemTime>) + Send + Sync>;

/// Thread-safe wakeup scheduler.
///
/// Any application, subsystem, or background task can schedule a future wake-up
/// when the system is in low-power Sleep mode.
pub struct WakeupScheduler {
    requests: Mutex<BTreeMap<String, WakeupRequest>>,
    listener: Mutex<Option<WakeupChangeListener>>,
}

impl std::fmt::Debug for WakeupScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WakeupScheduler")
            .field("requests", &self.requests)
            .finish()
    }
}

impl Default for WakeupScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl WakeupScheduler {
    /// Creates a new, empty wakeup scheduler.
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(BTreeMap::new()),
            listener: Mutex::new(None),
        }
    }

    /// Registers a listener callback invoked whenever the nearest scheduled wakeup time changes.
    pub fn set_change_listener<F>(&self, listener: F)
    where
        F: Fn(Option<SystemTime>) + Send + Sync + 'static,
    {
        let mut l = self.listener.lock().unwrap();
        *l = Some(Box::new(listener));
        let reqs = self.requests.lock().unwrap();
        let next_at = reqs.values().min_by_key(|req| req.at).map(|r| r.at);
        if let Some(cb) = l.as_ref() {
            cb(next_at);
        }
    }

    fn notify_change(&self, reqs: &BTreeMap<String, WakeupRequest>) {
        if let Ok(l) = self.listener.lock() {
            if let Some(cb) = l.as_ref() {
                let next_at = reqs.values().min_by_key(|req| req.at).map(|r| r.at);
                cb(next_at);
            }
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
        self.notify_change(&reqs);
    }

    /// Schedules a wakeup request after a given `Duration` from now.
    pub fn schedule_after(&self, id: impl Into<String>, delay: Duration, wake_screen: bool) {
        let at = SystemTime::now() + delay;
        self.schedule(id, at, wake_screen);
    }

    /// Cancels a scheduled wakeup by its `id`.
    /// Returns `true` if a request was found and cancelled.
    pub fn cancel(&self, id: &str) -> bool {
        let mut reqs = self.requests.lock().unwrap();
        let removed = reqs.remove(id).is_some();
        if removed {
            self.notify_change(&reqs);
        }
        removed
    }

    /// Clears all scheduled wakeup requests.
    pub fn clear(&self) {
        let mut reqs = self.requests.lock().unwrap();
        reqs.clear();
        self.notify_change(&reqs);
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

        for key in &keys_to_remove {
            reqs.remove(key);
        }

        if !expired.is_empty() {
            self.notify_change(&reqs);
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

    #[test]
    fn test_wakeup_change_listener() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let scheduler = WakeupScheduler::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let last_had_wakeup = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let calls_clone = Arc::clone(&calls);
        let flag_clone = Arc::clone(&last_had_wakeup);
        scheduler.set_change_listener(move |next_at| {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            flag_clone.store(next_at.is_some(), Ordering::SeqCst);
        });

        // 1. Initial call upon registration (None)
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!last_had_wakeup.load(Ordering::SeqCst));

        // 2. Schedule a future task
        scheduler.schedule("t1", SystemTime::now() + Duration::from_secs(10), true);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(last_had_wakeup.load(Ordering::SeqCst));

        // 3. Cancel task
        scheduler.cancel("t1");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(!last_had_wakeup.load(Ordering::SeqCst));
    }
}
