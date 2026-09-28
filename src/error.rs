//! Unified error type shared by every hardware backend.

use core::fmt;

/// Error returned by all HAL backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HalError {
    /// The peripheral or operation is not supported by this backend yet.
    NotSupported,
    /// The backend has not been initialized (or the subsystem is powered off).
    NotInitialized,
    /// The device is busy and cannot accept the request right now.
    Busy,
    /// The operation timed out.
    Timeout,
    /// An argument was invalid (e.g. empty SSID, unknown access point).
    InvalidArg,
    /// An I/O error, typically from the storage or transport layer.
    Io(String),
    /// A raw platform error code passed through unchanged (e.g. `esp_err_t`).
    Internal(i32),
}

impl HalError {
    /// Convert a C-style error code (`0` = success) into a `Result`.
    pub fn from_code(code: i32) -> Result<(), HalError> {
        if code == 0 {
            Ok(())
        } else {
            Err(HalError::Internal(code))
        }
    }
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HalError::NotSupported => write!(f, "operation not supported by this backend"),
            HalError::NotInitialized => write!(f, "backend is not initialized"),
            HalError::Busy => write!(f, "backend is busy"),
            HalError::Timeout => write!(f, "operation timed out"),
            HalError::InvalidArg => write!(f, "invalid argument"),
            HalError::Io(msg) => write!(f, "I/O error: {msg}"),
            HalError::Internal(code) => write!(f, "internal platform error (code {code})"),
        }
    }
}

impl std::error::Error for HalError {}
