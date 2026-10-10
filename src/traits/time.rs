use std::time::Duration;

use crate::error::HalError;

/// Platform-agnostic interface for system time and NTP synchronization.
pub trait TimeBackend: Send + Sync {
    /// Synchronize system time with an NTP/SNTP server.
    ///
    /// Long-running blocking operation on hardware — caller should run this on a background task/thread.
    /// `server`: optional NTP server address (e.g. "pool.ntp.org", None for default).
    /// `timeout`: maximum duration to wait for sync response.
    /// Returns the synchronized Unix timestamp (seconds since epoch) upon success.
    fn sync(&mut self, server: Option<&str>, timeout: Duration) -> Result<u64, HalError>;

    /// Returns whether the system time has been synchronized with an authoritative source (NTP).
    fn is_synced(&self) -> bool;

    /// Get current Unix timestamp in seconds.
    fn now_unix(&self) -> u64;
}
