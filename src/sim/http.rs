use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HalError;
use crate::traits::{HttpBackend, TimeBackend, VALID_TIME_EPOCH_2026};
use crate::types::{HttpState, SystemEvent};

#[cfg(not(target_os = "espidf"))]
enum FetchStatus {
    Running,
    Done { code: u16, data: Vec<u8> },
    Error { code: u16 },
}

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
    mock_now: Option<u64>,
    time_source: Option<Arc<Mutex<dyn TimeBackend>>>,
    ticks_remaining: usize,
    mock_responses: HashMap<String, (u16, Vec<u8>)>,
    mock_errors: HashSet<String>,
    pending_event: Option<SystemEvent>,
    #[cfg(not(target_os = "espidf"))]
    active_fetch: Option<std::sync::Arc<std::sync::Mutex<FetchStatus>>>,
}

impl SimHttp {
    pub fn new() -> Self {
        Self {
            state: HttpState::Idle,
            status_code: None,
            pending_payload: None,
            result: None,
            clock_synced: false,
            mock_now: None,
            time_source: None,
            ticks_remaining: 0,
            mock_responses: HashMap::new(),
            mock_errors: HashSet::new(),
            pending_event: None,
            #[cfg(not(target_os = "espidf"))]
            active_fetch: None,
        }
    }

    /// Set whether the simulator system clock is synchronized (for HTTPS validation).
    pub fn set_clock_synced(&mut self, synced: bool) {
        self.clock_synced = synced;
    }

    /// Link this HTTP backend to a shared time source.
    pub fn set_time_source(&mut self, time: Arc<Mutex<dyn TimeBackend>>) {
        self.time_source = Some(time);
    }

    /// Set or clear the mock timestamp for standalone testing.
    pub fn set_mock_time(&mut self, mock_now: Option<u64>) {
        self.mock_now = mock_now;
    }

    /// Returns whether the clock reading is valid (timestamp >= 2026-01-01).
    pub fn is_valid(&self) -> bool {
        self.now_unix() >= VALID_TIME_EPOCH_2026
    }

    /// Returns the current Unix timestamp in seconds.
    pub fn now_unix(&self) -> u64 {
        self.mock_now.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
    }

    /// Returns whether the clock is ready for HTTPS certificate validation:
    /// `is_synced() || is_valid()`
    pub fn is_clock_ready(&self) -> bool {
        if let Some(ref source) = self.time_source {
            if let Ok(guard) = source.lock() {
                return guard.is_synced() || guard.is_valid();
            }
        }
        self.clock_synced || self.is_valid()
    }

    /// Register a mock response for a given URL.
    pub fn set_mock_response(&mut self, url: impl Into<String>, status_code: u16, body: Vec<u8>) {
        self.mock_responses.insert(url.into(), (status_code, body));
    }

    /// Register a simulated error for a given URL.
    pub fn set_mock_error(&mut self, url: impl Into<String>) {
        self.mock_errors.insert(url.into());
    }

    #[cfg(not(target_os = "espidf"))]
    fn poll_active_fetch(&mut self) {
        let should_clear = if let Some(ref fetch) = self.active_fetch {
            if let Ok(st) = fetch.lock() {
                match &*st {
                    FetchStatus::Running => false,
                    FetchStatus::Done { code, data } => {
                        let len = data.len();
                        self.state = HttpState::Done;
                        self.status_code = Some(*code);
                        self.result = Some(data.clone());
                        self.pending_event = Some(SystemEvent::HttpFinished {
                            status_code: *code,
                            bytes: len,
                            error_code: None,
                        });
                        true
                    }
                    FetchStatus::Error { code } => {
                        self.state = HttpState::Error;
                        self.status_code = Some(*code);
                        self.pending_event = Some(SystemEvent::HttpFinished {
                            status_code: *code,
                            bytes: 0,
                            error_code: Some(*code as i32),
                        });
                        true
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if should_clear {
            self.active_fetch = None;
        }
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

        if url.starts_with("https://") && !self.is_clock_ready() {
            return Err(HalError::ClockNotSynced);
        }

        if matches!(self.state, HttpState::Running { .. }) {
            return Err(HalError::Busy);
        }

        self.result = None;
        self.status_code = None;

        let is_mock = self.mock_responses.contains_key(url)
            || self.mock_errors.contains(url)
            || url.contains("pomelo.os")
            || url.contains("example.");

        if is_mock {
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

            return Ok(());
        }

        // Real HTTP request on desktop
        #[cfg(not(target_os = "espidf"))]
        {
            let status = std::sync::Arc::new(std::sync::Mutex::new(FetchStatus::Running));
            self.active_fetch = Some(std::sync::Arc::clone(&status));
            self.state = HttpState::Running {
                downloaded: 0,
                total: None,
            };

            let target_url = url.to_string();
            std::thread::Builder::new()
                .name("sim-http-fetch".into())
                .spawn(move || {
                    use std::io::Read;
                    match ureq::get(&target_url)
                        .timeout(std::time::Duration::from_secs(10))
                        .call()
                    {
                        Ok(resp) => {
                            let code = resp.status();
                            let mut reader = resp.into_reader();
                            let mut bytes = Vec::new();
                            if reader.read_to_end(&mut bytes).is_ok() {
                                if let Ok(mut lock) = status.lock() {
                                    *lock = FetchStatus::Done { code, data: bytes };
                                }
                            } else if let Ok(mut lock) = status.lock() {
                                *lock = FetchStatus::Error { code: 500 };
                            }
                        }
                        Err(e) => {
                            let code = match e {
                                ureq::Error::Status(c, _) => c,
                                _ => 500,
                            };
                            if let Ok(mut lock) = status.lock() {
                                *lock = FetchStatus::Error { code };
                            }
                        }
                    }
                })
                .ok();

            Ok(())
        }

        #[cfg(target_os = "espidf")]
        Ok(())
    }

    fn state(&self) -> HttpState {
        #[cfg(not(target_os = "espidf"))]
        if let Some(ref fetch) = self.active_fetch {
            if let Ok(st) = fetch.lock() {
                match &*st {
                    FetchStatus::Running => return HttpState::Running { downloaded: 0, total: None },
                    FetchStatus::Done { .. } => return HttpState::Done,
                    FetchStatus::Error { .. } => return HttpState::Error,
                }
            }
        }
        self.state
    }

    fn status_code(&self) -> Option<u16> {
        #[cfg(not(target_os = "espidf"))]
        if let Some(ref fetch) = self.active_fetch {
            if let Ok(st) = fetch.lock() {
                match &*st {
                    FetchStatus::Done { code, .. } => return Some(*code),
                    FetchStatus::Error { code } => return Some(*code),
                    _ => {}
                }
            }
        }
        self.status_code
    }

    fn take_result(&mut self) -> Result<Vec<u8>, HalError> {
        #[cfg(not(target_os = "espidf"))]
        self.poll_active_fetch();

        if self.state != HttpState::Done {
            return Err(HalError::InvalidState);
        }

        let data = self.result.take().unwrap_or_default();
        self.state = HttpState::Idle;
        self.status_code = None;
        Ok(data)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        #[cfg(not(target_os = "espidf"))]
        {
            self.active_fetch = None;
        }
        self.state = HttpState::Idle;
        self.status_code = None;
        self.pending_payload = None;
        self.result = None;
        self.ticks_remaining = 0;
        Ok(())
    }

    fn tick(&mut self) {
        #[cfg(not(target_os = "espidf"))]
        self.poll_active_fetch();

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

    fn set_clock_synced(&mut self, synced: bool) {
        self.clock_synced = synced;
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
        // With mock cold-boot timestamp (e.g. 1970 epoch) and no sync, HTTPS is rejected
        http.set_mock_time(Some(0));
        assert_eq!(
            http.start("https://example.com/api"),
            Err(HalError::ClockNotSynced)
        );

        // Non-HTTPS starts fine
        assert!(http.start("http://example.com/api").is_ok());
    }

    #[test]
    fn sim_http_accepts_https_when_valid() {
        let mut http = SimHttp::new();
        // Host system time on modern systems is >= 2026, so is_valid() is true
        assert!(http.is_valid());
        assert!(http.start("https://example.com/api").is_ok());
        assert!(matches!(http.state(), HttpState::Running { .. }));
    }

    #[test]
    fn sim_http_accepts_https_when_synced() {
        let mut http = SimHttp::new();
        http.set_mock_time(Some(0)); // Even when clock is 1970 (is_valid == false)
        http.set_clock_synced(true); // But is_synced == true
        assert!(http.start("https://example.com/api").is_ok());
        assert!(matches!(http.state(), HttpState::Running { .. }));
    }

    #[test]
    fn sim_http_time_source_linkage() {
        use crate::sim::SimTime;

        let time = Arc::new(Mutex::new(SimTime::with_mock_time(0)));
        let mut http = SimHttp::new();
        http.set_time_source(Arc::clone(&time) as Arc<Mutex<dyn TimeBackend>>);

        // Time is 0 and unsynced -> HTTPS rejected
        assert_eq!(
            http.start("https://example.com/api"),
            Err(HalError::ClockNotSynced)
        );

        // Time synced -> HTTPS allowed
        time.lock().unwrap().sync(None, std::time::Duration::from_millis(10)).unwrap();
        assert!(http.start("https://example.com/api").is_ok());
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

    #[test]
    #[ignore = "requires real internet connection"]
    fn test_real_outlook_fetch() {
        let mut http = SimHttp::new();
        http.set_clock_synced(true);
        let res = http.start("https://outlook.live.com/owa/calendar/28feeeda-ca56-475f-88c5-c981c672bc26/d7960297-c084-43cc-b45b-e1539fde54fc/cid-59BB3F0030CA8F81/calendar.ics");
        println!("start result: {:?}", res);
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            println!("state: {:?}", http.state());
            if http.state() == HttpState::Done {
                let data = http.take_result().unwrap();
                println!("GOT DATA LEN: {}", data.len());
                let ics_str = String::from_utf8_lossy(&data);
                println!("FIRST 200 BYTES:\n{}", &ics_str[..200.min(ics_str.len())]);
                return;
            } else if http.state() == HttpState::Error {
                println!("GOT ERROR! Code: {:?}", http.status_code());
                panic!("Fetch returned error!");
            }
        }
        panic!("Timeout!");
    }
}
