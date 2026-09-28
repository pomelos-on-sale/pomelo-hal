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
        match self.rssi {
            r if r >= -55 => 4,
            r if r >= -65 => 3,
            r if r >= -75 => 2,
            r if r >= -85 => 1,
            _ => 0,
        }
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
