//! The Wi-Fi credentials this board keeps for itself.
//!
//! Persistent Wi-Fi credentials configuration implemented via [`crate::storage::PersistentConfig`]
//! and Flash NVS / Key-Value storage (`WIFI`/`credentials`).

use std::io;

use serde::{Deserialize, Serialize};

/// The Wi-Fi configuration and credentials schema.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WifiCredentials {
    /// Whether the radio is on at boot.
    #[serde(default)]
    pub enabled: bool,

    /// The network's name, as it is broadcast. Empty if no network has been configured yet.
    #[serde(default)]
    pub ssid: String,

    /// The password, in the clear.
    #[serde(default)]
    pub password: String,

    /// Whether the radio should connect to this network at boot without being asked.
    #[serde(default = "yes")]
    pub autoconnect: bool,
}

impl crate::storage::PersistentConfig for WifiCredentials {
    const APP_ID: &'static str = "WIFI";
    const KEY: &'static str = "credentials";
}

/// The default for `autoconnect`, as `serde` demands it: a function.
fn yes() -> bool {
    true
}

impl WifiCredentials {
    /// Loads the active Wi-Fi credentials from KV storage (falling back to Default if empty).
    pub fn load_active() -> Self {
        <Self as crate::storage::PersistentConfig>::load()
    }

    /// Saves the active Wi-Fi credentials to KV storage.
    pub fn save_active(&self) -> io::Result<()> {
        <Self as crate::storage::PersistentConfig>::save(self)
    }

    /// Clears the active Wi-Fi credentials in KV storage.
    pub fn forget_active() -> io::Result<()> {
        <Self as crate::storage::PersistentConfig>::forget()
    }

    /// Checks whether a network profile has been configured with a non-empty SSID.
    pub fn has_network(&self) -> bool {
        !self.ssid.is_empty()
    }

    /// Updates only the `enabled` field on the credentials and commits to KV storage.
    pub fn update_enabled(&mut self, enabled: bool) -> io::Result<()> {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.save_active()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> WifiCredentials {
        WifiCredentials {
            enabled: true,
            ssid: "Xiaomi_B2B4".into(),
            password: "hunter2".into(),
            autoconnect: true,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let written = credentials();
        let serialized = serde_json::to_string(&written).unwrap();
        let read: WifiCredentials = serde_json::from_str(&serialized).unwrap();
        assert_eq!(read, written);
    }

    #[test]
    fn a_password_with_spaces_or_quotes_survives() {
        for password in ["  padded  ", "\"\"", "a \"quoted\" word", "pass\\word"] {
            let written = WifiCredentials {
                password: password.into(),
                ..credentials()
            };
            let serialized = serde_json::to_string(&written).unwrap();
            let read: WifiCredentials = serde_json::from_str(&serialized).unwrap();
            assert_eq!(read, written);
        }
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"ssid":"home","password":"hunter2","priority":5,"unknown_table":{"foo":"bar"}}"#;
        let read: WifiCredentials = serde_json::from_str(json).unwrap();
        assert_eq!(
            read,
            WifiCredentials {
                enabled: false,
                ssid: "home".into(),
                password: "hunter2".into(),
                autoconnect: true,
            }
        );
    }

    #[test]
    fn default_values_applied() {
        let json = r#"{"ssid":"home"}"#;
        let read: WifiCredentials = serde_json::from_str(json).unwrap();
        assert!(!read.enabled);
        assert!(read.autoconnect);
        assert_eq!(read.password, "");
    }

    #[test]
    fn kv_lifecycle_load_save_forget() {
        let _ = WifiCredentials::forget_active();

        let initial = WifiCredentials::load_active();
        assert_eq!(initial, WifiCredentials::default());
        assert!(!initial.has_network());

        let creds = credentials();
        creds.save_active().unwrap();

        let loaded = WifiCredentials::load_active();
        assert_eq!(loaded, creds);
        assert!(loaded.has_network());

        let mut modifiable = loaded;
        modifiable.update_enabled(false).unwrap();
        assert_eq!(WifiCredentials::load_active().enabled, false);

        WifiCredentials::forget_active().unwrap();
        let after_forget = WifiCredentials::load_active();
        assert_eq!(after_forget, WifiCredentials::default());
    }
}
