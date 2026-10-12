use std::collections::{HashMap, HashSet};

use crate::error::HalError;
use crate::traits::HttpBackend;
use crate::types::{HttpState, SystemEvent};

/// Desktop simulator HTTP client backend.
///
/// Supports mock responses, error injection, clock synchronization guards,
/// and step-by-step tick progression for deterministic testing.
pub struct SimHttp {
    state: HttpState,
    status_code: Option<u16>,
    pending_payload: Option<Vec<u8>>,
    result: Option<Vec<u8>>,
    clock_synced: bool,
    ticks_remaining: usize,
    mock_responses: HashMap<String, (u16, Vec<u8>)>,
    mock_errors: HashSet<String>,
    pending_event: Option<SystemEvent>,
}

impl SimHttp {
    pub fn new() -> Self {
        Self {
            state: HttpState::Idle,
            status_code: None,
            pending_payload: None,
            result: None,
            clock_synced: false,
            ticks_remaining: 0,
            mock_responses: HashMap::new(),
            mock_errors: HashSet::new(),
            pending_event: None,
        }
    }

    /// Set whether the simulator system clock is synchronized (for HTTPS validation).
    pub fn set_clock_synced(&mut self, synced: bool) {
        self.clock_synced = synced;
    }

    /// Register a mock response for a given URL.
    pub fn set_mock_response(&mut self, url: impl Into<String>, status_code: u16, body: Vec<u8>) {
        self.mock_responses.insert(url.into(), (status_code, body));
    }

    /// Register a simulated error for a given URL.
    pub fn set_mock_error(&mut self, url: impl Into<String>) {
        self.mock_errors.insert(url.into());
    }
}

impl Default for SimHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpBackend for SimHttp {
    fn start(&mut self, url: &str) -> Result<(), HalError> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(HalError::InvalidArg);
        }

        if url.starts_with("https://") && !self.clock_synced {
            return Err(HalError::ClockNotSynced);
        }

        if matches!(self.state, HttpState::Running { .. }) {
            return Err(HalError::Busy);
        }

        self.result = None;
        self.status_code = None;

        if self.mock_errors.contains(url) {
            self.state = HttpState::Running {
                downloaded: 0,
                total: None,
            };
            self.status_code = Some(500);
            self.pending_payload = None;
            self.ticks_remaining = 1;
            return Ok(());
        }

        let (code, body) = if let Some((code, body)) = self.mock_responses.get(url) {
            (*code, body.clone())
        } else {
            (200, b"Simulated HTTP response payload".to_vec())
        };

        let total_len = body.len();
        self.status_code = Some(code);
        self.pending_payload = Some(body);
        self.ticks_remaining = 2;
        self.state = HttpState::Running {
            downloaded: 0,
            total: Some(total_len),
        };

        Ok(())
    }

    fn state(&self) -> HttpState {
        self.state
    }

    fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    fn take_result(&mut self) -> Result<Vec<u8>, HalError> {
        if self.state != HttpState::Done {
            return Err(HalError::InvalidState);
        }

        let data = self.result.take().unwrap_or_default();
        self.state = HttpState::Idle;
        self.status_code = None;
        Ok(data)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        self.state = HttpState::Idle;
        self.status_code = None;
        self.pending_payload = None;
        self.result = None;
        self.ticks_remaining = 0;
        Ok(())
    }

    fn tick(&mut self) {
        if let HttpState::Running { total, .. } = self.state {
            if self.ticks_remaining > 0 {
                self.ticks_remaining -= 1;
            }

            if self.ticks_remaining == 1 {
                let downloaded = total.unwrap_or(0) / 2;
                self.state = HttpState::Running { downloaded, total };
            } else if self.ticks_remaining == 0 {
                if let Some(payload) = self.pending_payload.take() {
                    let len = payload.len();
                    self.result = Some(payload);
                    self.state = HttpState::Done;
                    let code = self.status_code.unwrap_or(200);
                    self.pending_event = Some(SystemEvent::HttpFinished {
                        status_code: code,
                        bytes: len,
                        error_code: None,
                    });
                } else {
                    self.state = HttpState::Error;
                    let code = self.status_code.unwrap_or(500);
                    self.pending_event = Some(SystemEvent::HttpFinished {
                        status_code: code,
                        bytes: 0,
                        error_code: Some(code as i32),
                    });
                }
            }
        }
    }

    fn poll_event(&mut self) -> Option<SystemEvent> {
        self.pending_event.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_http_rejects_https_without_clock_sync() {
        let mut http = SimHttp::new();
        assert_eq!(
            http.start("https://example.com/api"),
            Err(HalError::ClockNotSynced)
        );

        // Non-HTTPS starts fine
        assert!(http.start("http://example.com/api").is_ok());
    }

    #[test]
    fn sim_http_accepts_https_when_synced() {
        let mut http = SimHttp::new();
        http.set_clock_synced(true);
        assert!(http.start("https://example.com/api").is_ok());
        assert!(matches!(http.state(), HttpState::Running { .. }));
    }

    #[test]
    fn sim_http_lifecycle_mock_response() {
        let mut http = SimHttp::new();
        http.set_mock_response("http://test.local/data.json", 200, b"{\"ok\":true}".to_vec());

        http.start("http://test.local/data.json").unwrap();
        assert!(matches!(http.state(), HttpState::Running { .. }));

        // Advance ticks
        http.tick();
        assert!(matches!(http.state(), HttpState::Running { .. }));
        http.tick();
        assert_eq!(http.state(), HttpState::Done);
        assert_eq!(http.status_code(), Some(200));

        let data = http.take_result().unwrap();
        assert_eq!(data, b"{\"ok\":true}");
        assert_eq!(http.state(), HttpState::Idle);
    }

    #[test]
    fn sim_http_cancel_resets_to_idle() {
        let mut http = SimHttp::new();
        http.start("http://test.local/long").unwrap();
        assert!(matches!(http.state(), HttpState::Running { .. }));

        http.cancel().unwrap();
        assert_eq!(http.state(), HttpState::Idle);
        assert_eq!(http.take_result(), Err(HalError::InvalidState));
    }

    #[test]
    fn sim_http_error_simulation() {
        let mut http = SimHttp::new();
        http.set_mock_error("http://test.local/fail");

        http.start("http://test.local/fail").unwrap();
        http.tick();
        assert_eq!(http.state(), HttpState::Error);
    }
}
