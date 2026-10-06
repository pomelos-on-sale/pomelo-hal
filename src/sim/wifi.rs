//! Simulated Wi-Fi radio with a deterministic, tick-driven scan state machine.

use crate::error::HalError;
use crate::traits::WifiBackend;
use crate::types::{ApInfo, ScanState, WifiState, WifiStatus};
use crate::wifi_credentials::WifiCredentials;

const SIM_APS: &[(&str, i8, bool, u8)] = &[
    ("ESP-Rust-5G", -46, true, 36),
    ("Pomelo-OS", -58, true, 6),
    ("HomeNet_2.4G", -67, true, 11),
    ("OpenGuest", -74, false, 1),
    ("IoT_Devices", -82, true, 44),
];

fn sim_ap_list() -> Vec<ApInfo> {
    SIM_APS
        .iter()
        .map(|(ssid, rssi, secure, ch)| ApInfo::new(*ssid, *rssi, *secure, *ch))
        .collect()
}

/// Simulated Wi-Fi radio.
pub struct SimWifi {
    enabled: bool,
    scan: ScanState,
    scan_ticks_left: u8,
    connected: Option<(String, i8)>,
    jitter: i32,
    /// In memory, and deliberately not on disk.
    ///
    /// A simulator that wrote to the person's home directory when they ran the test suite would make
    /// the tests depend on the machine and the machine depend on the tests. The file itself is
    /// exercised by `wifi_credentials`'s own tests, which use a directory of their own under the
    /// system's temp directory; what the simulator has to get right is the *trait*, not `read_to_string`.
    saved: Option<WifiCredentials>,
}

impl SimWifi {
    pub fn new() -> Self {
        Self {
            enabled: true,
            scan: ScanState::Idle,
            scan_ticks_left: 0,
            connected: None,
            jitter: 0,
            saved: None,
        }
    }
}

impl Default for SimWifi {
    fn default() -> Self {
        Self::new()
    }
}

impl WifiBackend for SimWifi {
    fn saved(&self) -> Option<WifiCredentials> {
        self.saved.clone()
    }

    fn remember(&mut self, credentials: &WifiCredentials) -> Result<(), HalError> {
        self.saved = Some(credentials.clone());
        Ok(())
    }

    fn forget(&mut self) -> Result<(), HalError> {
        self.saved = None;
        Ok(())
    }

    fn init(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    /// The same reading of the file the board does, so that the two do not drift apart.
    ///
    /// Which is the whole reason the simulator has it: the boot behaviour is a rule about a file,
    /// and a rule only one side implements is a rule only testable on the side nobody can run here.
    fn autoconnect(&mut self) -> Result<(), HalError> {
        let Some(saved) = self.saved() else {
            return Ok(());
        };

        if !saved.enabled {
            return Ok(());
        }

        self.set_enabled(true)?;

        if !saved.autoconnect {
            return Ok(());
        }

        self.connect(&saved.ssid, &saved.password)
    }

    fn set_enabled(&mut self, on: bool) -> Result<(), HalError> {
        self.enabled = on;
        if !on {
            self.scan = ScanState::Idle;
            self.scan_ticks_left = 0;
            self.connected = None;
        }
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn scan_start(&mut self) -> Result<(), HalError> {
        if !self.enabled {
            return Err(HalError::NotInitialized);
        }
        self.scan = ScanState::Scanning;
        self.scan_ticks_left = 3;
        Ok(())
    }

    fn scan_state(&self) -> ScanState {
        self.scan
    }

    fn scan_results(&self) -> Result<Vec<ApInfo>, HalError> {
        match self.scan {
            ScanState::Done => Ok(sim_ap_list()),
            _ => Err(HalError::Busy),
        }
    }

    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), HalError> {
        if !self.enabled {
            return Err(HalError::NotInitialized);
        }
        let ap = SIM_APS
            .iter()
            .find(|(name, ..)| *name == ssid)
            .ok_or(HalError::InvalidArg)?;
        if ap.2 && password.is_empty() {
            return Err(HalError::InvalidArg);
        }
        self.connected = Some((ssid.to_string(), ap.1));
        self.scan = ScanState::Idle;
        self.scan_ticks_left = 0;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), HalError> {
        self.connected = None;
        Ok(())
    }

    fn status(&self) -> WifiStatus {
        match &self.connected {
            Some((ssid, base_rssi)) => WifiStatus {
                state: WifiState::Connected,
                ssid: ssid.clone(),
                ip: "192.168.1.108".to_string(),
                netmask: "255.255.255.0".to_string(),
                gateway: "192.168.1.1".to_string(),
                rssi: (*base_rssi as i32 + self.jitter).clamp(-127, 0) as i8,
            },
            None => WifiStatus {
                state: if self.scan == ScanState::Scanning {
                    WifiState::Scanning
                } else {
                    WifiState::Disconnected
                },
                ..Default::default()
            },
        }
    }

    fn tick(&mut self) {
        if self.scan == ScanState::Scanning {
            self.scan_ticks_left = self.scan_ticks_left.saturating_sub(1);
            if self.scan_ticks_left == 0 {
                self.scan = ScanState::Done;
            }
        }
        if self.connected.is_some() {
            self.jitter = if self.jitter >= 2 {
                -2
            } else {
                self.jitter + 1
            };
        }
    }
}
