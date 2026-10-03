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
    /// User input action.
    InputAction(InputAction),
}


