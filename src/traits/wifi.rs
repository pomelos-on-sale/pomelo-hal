//! Wi-Fi interface.

use crate::error::HalError;
use crate::types::{ApInfo, ScanState, WifiStatus};

/// Wi-Fi station management.
pub trait WifiBackend: Send + Sync {
    fn init(&mut self) -> Result<(), HalError>;

    /// Enable or disable the radio.
    fn set_enabled(&mut self, on: bool) -> Result<(), HalError>;
    fn is_enabled(&self) -> bool;

    /// Begin an asynchronous scan for nearby access points.
    fn scan_start(&mut self) -> Result<(), HalError>;
    /// Current scan progress.
    fn scan_state(&self) -> ScanState;
    /// Results of the most recent completed scan.
    fn scan_results(&self) -> Result<Vec<ApInfo>, HalError>;

    /// Connect to an access point. `password` may be empty for open networks.
    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), HalError>;
    fn disconnect(&mut self) -> Result<(), HalError>;
    /// Current connection status snapshot.
    fn status(&self) -> WifiStatus;

    /// Advance simulated asynchronous state. Native event-driven backends can
    /// ignore this (default no-op).
    fn tick(&mut self) {}
}
