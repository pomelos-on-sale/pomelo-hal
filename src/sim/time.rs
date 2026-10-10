use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::HalError;
use crate::traits::TimeBackend;

/// Simulated time and NTP backend for desktop environments.
pub struct SimTime {
    synced: bool,
}

impl SimTime {
    pub fn new() -> Self {
        Self { synced: false }
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
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_time_initial_and_sync() {
        let mut time = SimTime::new();
        assert!(!time.is_synced());
        assert!(time.now_unix() > 0);

        let res = time.sync(None, Duration::from_secs(1)).unwrap();
        assert!(time.is_synced());
        assert_eq!(res, time.now_unix());
    }
}
