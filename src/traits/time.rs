use std::time::Duration;

use crate::error::HalError;

/// Minimum Unix timestamp considered plausible and valid for modern operation (2026-01-01 00:00:00 UTC).
pub const VALID_TIME_EPOCH_2026: u64 = 1_767_225_600;

/// Platform-agnostic interface for system time and NTP synchronization.
pub trait TimeBackend: Send + Sync {
    /// Synchronize system time with an NTP/SNTP server.
    ///
    /// Long-running blocking operation on hardware — caller should run this on a background task/thread.
    /// `server`: optional NTP server address (e.g. "pool.ntp.org", None for default).
    /// `timeout`: maximum duration to wait for sync response.
    /// Returns the synchronized Unix timestamp (seconds since epoch) upon success.
    fn sync(&mut self, server: Option<&str>, timeout: Duration) -> Result<u64, HalError>;

    /// Returns whether the system time has been explicitly synchronized with an authoritative source (NTP).
    fn is_synced(&self) -> bool;

    /// Get current Unix timestamp in seconds.
    fn now_unix(&self) -> u64;

    /// Returns whether the current system clock reading is plausible and valid for practical
    /// operations (such as TLS certificate verification), i.e. timestamp >= 2026-01-01.
    fn is_valid(&self) -> bool {
        self.now_unix() >= VALID_TIME_EPOCH_2026
    }
}
