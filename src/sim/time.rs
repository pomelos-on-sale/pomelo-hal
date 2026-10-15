use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::HalError;
use crate::traits::TimeBackend;

/// Simulated time and NTP backend for desktop environments.
pub struct SimTime {
    synced: bool,
    mock_now: Option<u64>,
}

impl SimTime {
    pub fn new() -> Self {
        Self {
            synced: false,
            mock_now: None,
        }
    }

    /// Construct a simulator time instance with a fixed mock timestamp.
    pub fn with_mock_time(now_unix: u64) -> Self {
        Self {
            synced: false,
            mock_now: Some(now_unix),
        }
    }

    /// Set or clear the mock timestamp. Passing `None` restores host system time.
    pub fn set_mock_time(&mut self, now_unix: Option<u64>) {
        self.mock_now = now_unix;
    }
}

impl Default for SimTime {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeBackend for SimTime {
    fn sync(&mut self, _server: Option<&str>, _timeout: Duration) -> Result<u64, HalError> {
        self.synced = true;
        Ok(self.now_unix())
    }

    fn is_synced(&self) -> bool {
        self.synced
    }

    fn now_unix(&self) -> u64 {
        self.mock_now.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::VALID_TIME_EPOCH_2026;

    #[test]
    fn sim_time_initial_and_sync() {
        let mut time = SimTime::new();
        assert!(!time.is_synced());
        assert!(time.now_unix() > 0);

        let res = time.sync(None, Duration::from_secs(1)).unwrap();
        assert!(time.is_synced());
        assert_eq!(res, time.now_unix());
    }

    #[test]
    fn sim_time_is_valid_semantics() {
        let mut time = SimTime::new();
        // Host system time on modern systems is >= 2026
        assert!(time.is_valid());
        assert!(!time.is_synced());

        // Cold boot unsynchronized time (e.g. 1970 epoch)
        time.set_mock_time(Some(0));
        assert_eq!(time.now_unix(), 0);
        assert!(!time.is_valid());
        assert!(!time.is_synced());

        // Valid timestamp in 2026
        time.set_mock_time(Some(VALID_TIME_EPOCH_2026));
        assert!(time.is_valid());
        assert!(!time.is_synced());

        // Syncing marks is_synced true regardless of epoch
        time.set_mock_time(Some(0));
        assert!(!time.is_valid());
        time.sync(None, Duration::from_millis(10)).unwrap();
        assert!(time.is_synced());
    }
}
