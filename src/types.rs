//! Strongly-typed data structures shared across HAL backends.

/// Wi-Fi scan progress reported by [`crate::traits::WifiBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanState {
    /// No scan has been started, or the previous results were consumed.
    #[default]
    Idle,
    /// A scan is currently in progress.
    Scanning,
    /// The scan completed and results are ready.
    Done,
    /// The scan failed.
    Error,
}

/// High-level Wi-Fi connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiState {
    #[default]
    Disconnected,
    Scanning,
    Connecting,
    Connected,
}

/// A discovered Wi-Fi access point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApInfo {
    pub ssid: String,
    pub rssi: i8,
    pub secure: bool,
    pub channel: u8,
}

impl ApInfo {
    pub fn new(ssid: impl Into<String>, rssi: i8, secure: bool, channel: u8) -> Self {
        Self {
            ssid: ssid.into(),
            rssi,
            secure,
            channel,
        }
    }

    /// Signal quality mapped to `0..=4` bars, matching the status-bar icon.
    pub fn signal_bars(&self) -> u8 {
        signal_bars(self.rssi)
    }
}

/// Signal strength mapped to `0..=4` bars.
///
/// The one place the scale is written: a network's row and the status bar both ask a device or a
/// connection for its bars, so a scale with two owners cannot drift.
pub fn signal_bars(rssi: i8) -> u8 {
    match rssi {
        r if r >= -55 => 4,
        r if r >= -65 => 3,
        r if r >= -75 => 2,
        r if r >= -85 => 1,
        _ => 0,
    }
}

/// Current Wi-Fi status snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiStatus {
    pub state: WifiState,
    pub ssid: String,
    pub ip: String,
    pub netmask: String,
    pub gateway: String,
    pub rssi: i8,
}

impl WifiStatus {
    /// Signal quality of the connection, in `0..=4` bars — the same scale as
    /// [`ApInfo::signal_bars`].
    ///
    /// Zero when there is no connection: `rssi` only means something while one exists, and a status
    /// that was never filled in has `rssi` 0, which would otherwise read as a perfect signal.
    pub fn signal_bars(&self) -> u8 {
        if self.state != WifiState::Connected {
            return 0;
        }

        signal_bars(self.rssi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_scale_is_monotone() {
        // The thresholds, from best to none, and one sample either side of each.
        let samples = [
            (-40, 4),
            (-55, 4),
            (-56, 3),
            (-65, 3),
            (-66, 2),
            (-75, 2),
            (-76, 1),
            (-85, 1),
            (-86, 0),
            (-127, 0),
        ];

        for (rssi, bars) in samples {
            assert_eq!(signal_bars(rssi), bars, "{rssi} dBm");
        }
    }

    #[test]
    fn a_status_without_a_connection_has_no_bars() {
        // `rssi` is 0 in a default status, which on the bare scale is a perfect signal.
        assert_eq!(WifiStatus::default().signal_bars(), 0);

        let connected = WifiStatus {
            state: WifiState::Connected,
            rssi: -60,
            ..WifiStatus::default()
        };

        assert_eq!(connected.signal_bars(), 3);
    }
}

/// Metadata describing an audio stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioMeta {
    pub sample_rate: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
    pub duration_secs: f32,
}

/// A simple three-component vector used for IMU readings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

/// An abstract user input action (physical button, gesture, or navigation command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    /// Back navigation: return to previous page/screen, close modal, or keep app in background.
    Back,
    /// Exit / Kill: terminate the current app and release its memory.
    Exit,
    /// Home navigation: return to the launcher home screen without killing the active app.
    Home,
}

/// Current status of the SD / TF card slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SdCardStatus {
    /// No card is detected in the slot.
    #[default]
    NotInserted,
    /// Physical card detected, but filesystem is unformatted or unsupported (e.g. exFAT).
    Unformatted,
    /// Card is inserted and successfully mounted as a filesystem.
    Mounted,
    /// Card is present or encountered an error while communicating.
    Error,
}

/// Hardware identity and metadata of the SD / TF card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdCardInfo {
    /// Product model name (e.g. "SL64G" or "SanDisk").
    pub name: String,
    /// Physical storage capacity in bytes (from CSD register).
    pub total_bytes: u64,
    /// Hardware serial number (from CID register).
    pub serial_number: u32,
    /// Whether the filesystem on the card is currently recognized and mounted.
    pub is_formatted: bool,
}

/// Storage capacity and usage statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StorageStats {
    /// Total storage capacity in bytes.
    pub total_bytes: u64,
    /// Free storage space in bytes.
    pub free_bytes: u64,
}

impl StorageStats {
    pub const fn new(total_bytes: u64, free_bytes: u64) -> Self {
        Self {
            total_bytes,
            free_bytes,
        }
    }

    /// Storage space used in bytes.
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// Used percentage in `0.0..=100.0`.
    pub fn used_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes() as f64 / self.total_bytes as f64 * 100.0) as f32
        }
    }
}

/// Statistics from a system data backup or restore operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackupStats {
    /// Number of files copied.
    pub files_count: usize,
    /// Total bytes copied.
    pub bytes_count: u64,
}

impl BackupStats {
    pub const fn new(files_count: usize, bytes_count: u64) -> Self {
        Self {
            files_count,
            bytes_count,
        }
    }
}

/// A hardware or system event emitted by the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEvent {
    /// Battery or power supply status changed.
    BatteryChanged {
        percent: u8,
        charging: bool,
        voltage_mv: u32,
    },
    /// Wi-Fi connection status or signal changed.
    WifiStatusChanged(WifiStatus),
    /// System time successfully synchronized with network (SNTP).
    TimeSynced {
        unix_secs: u64,
    },
    /// User input action.
    InputAction(InputAction),
    /// SD card status changed (inserted, mounted, removed, or error).
    SdCardChanged(SdCardStatus),
    /// Asynchronous HTTP request completed (either success or error).
    HttpFinished {
        status_code: u16,
        bytes: usize,
        error_code: Option<i32>,
    },
    /// A scheduled wakeup timer fired.
    WakeupTriggered {
        id: String,
        wake_screen: bool,
    },
    /// Periodic system minute tick, emitted precisely when wall-clock second reaches 00.
    MinuteTick {
        unix_secs: u64,
    },
    /// Periodic system second tick, emitted every second.
    SecondTick {
        unix_secs: u64,
    },
}

/// The reason why the system woke up from low-power sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupReason {
    None,
    Timer,
    Gpio,
    Touch,
    Unknown,
}

/// HTTP request / download status reported by [`crate::traits::HttpBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpState {
    /// No request is active or previous result was consumed.
    #[default]
    Idle,
    /// Request is running in background.
    Running {
        /// Number of bytes downloaded so far.
        downloaded: usize,
        /// Total content length in bytes, if reported by server.
        total: Option<usize>,
    },
    /// Request finished successfully with data ready to take.
    Done,
    /// Request failed (network error, TLS failure, HTTP 4xx/5xx, etc.).
    Error,
}
