//! Microphone capture interface.

use crate::error::HalError;

/// Microphone capture.
pub trait MicBackend: Send + Sync {
    fn record_start(&mut self) -> Result<(), HalError>;
    /// Fill `buf` with captured samples, returning the number of samples written.
    fn read(&mut self, buf: &mut [i16]) -> Result<usize, HalError>;
    fn record_stop(&mut self) -> Result<(), HalError>;
    fn is_recording(&self) -> bool;
}
