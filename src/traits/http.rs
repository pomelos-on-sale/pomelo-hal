use crate::error::HalError;
use crate::types::HttpState;

/// Platform-agnostic interface for HTTP/HTTPS network requests.
///
/// Long-running requests follow a *start + poll* pattern:
/// - [`HttpBackend::start`] initiates an asynchronous request.
/// - [`HttpBackend::state`] polls the current progress without blocking.
/// - [`HttpBackend::take_result`] retrieves the response payload when state is [`HttpState::Done`].
pub trait HttpBackend: Send + Sync {
    /// Initiate an asynchronous GET request for `url`.
    ///
    /// If `url` starts with `https://` and system time is not synchronized with NTP
    /// (which causes TLS certificate verification failure), this immediately returns
    /// `Err(HalError::ClockNotSynced)`.
    fn start(&mut self, url: &str) -> Result<(), HalError>;

    /// Poll current request state without blocking.
    fn state(&self) -> HttpState;

    /// Return the HTTP status code (e.g. 200, 404) if headers have been received.
    fn status_code(&self) -> Option<u16> {
        None
    }

    /// Retrieve the response payload and reset the state back to [`HttpState::Idle`].
    ///
    /// Returns `Err(HalError::InvalidState)` if request is not completed yet.
    fn take_result(&mut self) -> Result<Vec<u8>, HalError>;

    /// Abort an in-flight request and reset state to [`HttpState::Idle`].
    fn cancel(&mut self) -> Result<(), HalError>;

    /// Advance time-driven simulator/mock state if necessary.
    fn tick(&mut self) {}

    /// Poll for an asynchronous hardware or simulator event emitted by this backend, if any.
    fn poll_event(&mut self) -> Option<crate::types::SystemEvent> {
        None
    }
}
