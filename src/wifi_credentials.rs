//! The Wi-Fi credentials this board keeps for itself.
//!
//! # Why the driver is not the one keeping them
//!
//! It does, today: `WIFI_STORAGE_FLASH` is IDF's default and `board_wifi.c` never overrides it, so
//! every `esp_wifi_set_config` writes the SSID and the password into the `nvs.net80211` namespace.
//! The cost of leaving it there is not the storage — it is that the board then has two opinions
//! about which network it belongs to and no way to reconcile them. The driver's copy is readable
//! only through `esp_wifi_get_config`, has no room for anything the *app* wants to remember
//! ("connect here at boot"), and is rewritten on every attempt, **including the failed ones** — so
//! a mistyped password destroys the one that worked.
//!
//! So the app owns them, and the driver is told to stop keeping a copy: `WIFI_STORAGE_RAM`, set once
//! in `board_wifi.c` where the rest of the station is configured.
//!
//! # The format
//!
//! `key=value` in sections — the shape NetworkManager uses for Wi-Fi profiles
//! (`/etc/NetworkManager/system-connections/*.nmconnection`), which is what "a modern Ubuntu config
//! file" means for exactly this job.
//!
//! TOML, through `serde`: the struct below *is* the schema, `toml::from_str` is the reader and
//! `toml::to_string` the writer, and adding a setting is adding a field. That was not affordable when
//! this file was first written — the firmware looked like it had a fifth of the partition left — but
//! it has 2.1 MB, and one `derive` is a better trade than a parser per file.
//!
//! The cost is that the file belongs to the machine: a save rewrites it, so a comment a person left
//! in it does not survive. If something needs saying to the next reader, it goes in this comment
//! block, which is source.
//!
//! ```text
//! # /internal/AppData/WIFI/wifi.conf
//! [wifi]
//! enabled = true
//! ssid = "Xiaomi_B2B4"
//! password = "hunter2"
//! autoconnect = true
//! ```
//!
//! Unknown sections and unknown keys are **ignored and not refused**: a file written by a later
//! version of this firmware has to stay readable by an earlier one, or a downgrade costs you the
//! radio.
//!
//! # In the clear
//!
//! The password is written as text and read as text, because that is what it is on this board
//! either way: neither `CONFIG_NVS_ENCRYPTION` nor `CONFIG_FLASH_ENCRYPTION_ENABLED` is set, so the
//! NVS copy this replaces was no better protected. Anyone who can read the flash can read it —
//! which is a thing to decide about deliberately, and the reason this file quotes rather than
//! mangles its values: a password nobody can spell is worse than one everybody can read.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file's name, inside the app's Wi-Fi directory.
pub const FILE: &str = "wifi.conf";

/// The directory the Wi-Fi files live in, inside an app-data root.
pub const DIRECTORY: &str = "WIFI";

/// The app-data root on the board: the `internal` partition, mounted by the firmware.
///
/// `/internal` is a 3 MB SPIFFS partition (`partitions.csv`), and mounting it is the firmware's job,
/// not this crate's. A board where nothing mounted it fails on the *write*, not on the read: a file
/// that is not there reads as `Ok(None)`, because "never been on a network" and "nowhere to put
/// one" must not look the same — only the second is a fault, and only the second is the
/// firmware's.
pub const BOARD_APP_DATA: &str = "/internal/AppData";

/// The app-data root on a desktop, under the home directory.
///
/// A desktop has no partition to mount, so the same shape sits one directory down instead. The
/// simulator does not use it — see `sim::wifi` for why — but it is here so that the two platforms
/// are one line apart rather than one design apart.
pub const DESKTOP_APP_DATA: &str = ".pomelo/AppData";

/// Where the file is, under `root`.
pub fn path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(DIRECTORY).join(FILE)
}

/// The `[wifi]` section of the board's Wi-Fi file.
///
/// The `derive` is the schema: adding a setting to this board's Wi-Fi is adding a field here, and the
/// reader, the writer and the check that the file matches all follow from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiCredentials {
    /// Whether the radio is on at boot.
    ///
    /// Defaults to *off*: a file that predates the key is a board whose radio came up off, and a
    /// default that changes what the board already did is not a default, it is a change.
    #[serde(default)]
    pub enabled: bool,

    /// The network's name, as it is broadcast. Not translated and not owned by us: it is the
    /// network's own string.
    ///
    /// **No default.** A file that names no network is not a Wi-Fi file, and a `String` would happily
    /// default to `""` and then connect to nothing without saying so.
    pub ssid: String,

    /// The password, in the clear. See the note at the top of the module.
    #[serde(default)]
    pub password: String,

    /// Whether the radio should connect to this network at boot without being asked.
    ///
    /// Defaults to *on* — the opposite of `enabled`, and not a contradiction: one is about the radio
    /// being on, the other about what it does unasked. A file that predates this key was written by
    /// someone who was connecting.
    #[serde(default = "yes")]
    pub autoconnect: bool,
}

/// The default for `autoconnect`, as `serde` demands it: a function, because an attribute holds a
/// path and not an expression.
fn yes() -> bool {
    true
}

/// The whole file: one table.
///
/// It exists so that a table this version does not know about is a field this struct does not have —
/// and `serde` ignores those, which is the whole of what keeps a downgrade working.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    wifi: WifiCredentials,
}

impl WifiCredentials {
    /// Reads the file's text.
    ///
    /// A missing `[wifi]`, a missing `ssid`, a value of the wrong type — every one of them is an
    /// error here, on the line it is on. That is what a typed file buys: the failure happens at the
    /// file, named, instead of three screens away at a radio that will not connect and cannot say
    /// why.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Document>(text).map(|document| document.wifi)
    }

    /// The file's text, ready to write.
    ///
    /// Fallible in principle — `serde` can fail to write a type it has no representation for — and
    /// infallible in fact for four plain fields. The signature says "in principle" rather than
    /// promising what it cannot promise.
    pub fn to_file(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&Document {
            wifi: self.clone(),
        })
    }

    /// Reads the file under `root`, if there is one.
    ///
    /// `Ok(None)` for a board that has never been on a network, which is not a failure. A file that
    /// is *there* and unreadable is one, and comes back as `InvalidData` carrying the reason — so
    /// "you have never connected" and "your file is corrupt" stay two different screens.
    pub fn load(root: impl AsRef<Path>) -> io::Result<Option<Self>> {
        let text = match fs::read_to_string(path(root)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };

        Self::parse(&text)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Writes the file under `root`, making the directories on the way.
    pub fn save(&self, root: impl AsRef<Path>) -> io::Result<()> {
        let path = path(root);
        let text = self
            .to_file()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, text)
    }

    /// Removes the file under `root`. Not an error if there was nothing to remove — the caller
    /// asked for the network to be forgotten, and it is.
    pub fn forget(root: impl AsRef<Path>) -> io::Result<()> {
        match fs::remove_file(path(root)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

// No error type of our own. `toml::de::Error` already says what is wrong and on which line, and
// wrapping it to say the same thing in this crate's words would cost the line number to say it —
// which is the one part of the message worth having.

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
    fn a_file_written_is_a_file_read() {
        let written = credentials();

        assert_eq!(
            WifiCredentials::parse(&written.to_file().unwrap()).unwrap(),
            written,
            "the writer and the reader are the same `derive`, which is the point of it"
        );
    }

    #[test]
    fn a_password_with_spaces_or_quotes_survives() {
        for password in ["  padded  ", "\"\"", "a \"quoted\" word", "pass\\word"] {
            let written = WifiCredentials {
                password: password.into(),
                ..credentials()
            };

            assert_eq!(
                WifiCredentials::parse(&written.to_file().unwrap()).unwrap(),
                written,
                "{password:?} came back changed"
            );
        }
    }

    #[test]
    fn a_table_a_later_version_wrote_is_ignored() {
        let read = WifiCredentials::parse(
            "[wifi]\n\
             ssid = \"home\"\n\
             password = \"hunter2\"\n\
             priority = 5\n\
             \n\
             [ethernet]\n\
             method = \"auto\"\n",
        )
        .unwrap();

        assert_eq!(
            read,
            WifiCredentials {
                enabled: false,
                ssid: "home".into(),
                password: "hunter2".into(),
                autoconnect: true,
            },
            "an unknown key or table is a field this struct does not have, which `serde` ignores"
        );
    }

    #[test]
    fn the_switches_have_the_defaults_that_keep_a_board_quiet() {
        let without = WifiCredentials::parse("[wifi]\nssid = \"home\"\n").unwrap();

        assert!(
            !without.enabled,
            "a file written before the key existed is a board whose radio came up off"
        );
        assert!(
            without.autoconnect,
            "and one whose owner was connecting: the two defaults point opposite ways on purpose"
        );
        assert_eq!(
            without.password, "",
            "no password is an open network, not a failure"
        );
    }

    #[test]
    fn a_file_with_no_network_in_it_is_an_error() {
        let error = WifiCredentials::parse("[wifi]\npassword = \"hunter2\"\n").unwrap_err();

        assert!(
            error.to_string().contains("ssid"),
            "and the message names the field that is missing: {error}"
        );
        assert!(
            WifiCredentials::parse("").is_err(),
            "a file with no `[wifi]` is an error too, and not an empty network"
        );
    }

    #[test]
    fn a_value_of_the_wrong_type_is_an_error_and_not_a_default() {
        let error =
            WifiCredentials::parse("[wifi]\nssid = \"home\"\nautoconnect = \"yes\"\n").unwrap_err();

        assert!(
            error.to_string().contains("boolean"),
            "`yes` is a string and not a `true`, and a typed file says so instead of guessing: {error}"
        );
    }

    /// A directory of this test's own, under the system's temp directory.
    ///
    /// Named after the test, so that two of them running at once do not share a file — and so that a
    /// failure leaves its evidence behind to be looked at.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join("pomelo-hal-wifi-credentials")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn nothing_saved_is_not_a_failure() {
        let root = temp_root("nothing");

        assert_eq!(
            WifiCredentials::load(&root).unwrap(),
            None,
            "a board that has never been on a network"
        );
        assert!(
            WifiCredentials::forget(&root).is_ok(),
            "and forgetting a network that was never there is not an error either"
        );
    }

    #[test]
    fn a_saved_network_comes_back() {
        let root = temp_root("round-trip");
        let written = credentials();

        written.save(&root).unwrap();

        assert_eq!(
            WifiCredentials::load(&root).unwrap(),
            Some(written),
            "through the filesystem, not only through the parser"
        );
        assert!(
            path(&root).ends_with("WIFI/wifi.conf"),
            "and it went where the constant says: {:?}",
            path(&root)
        );
    }

    #[test]
    fn forgetting_removes_the_file() {
        let root = temp_root("forget");

        credentials().save(&root).unwrap();
        WifiCredentials::forget(&root).unwrap();

        assert_eq!(WifiCredentials::load(&root).unwrap(), None);
    }

    #[test]
    fn a_file_that_is_there_and_wrong_says_so() {
        let root = temp_root("malformed");
        let file = path(&root);

        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "[wifi]\nnot a pair\n").unwrap();

        let error = WifiCredentials::load(&root).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("line 2"),
            "and the message points at the line: {error}"
        );
    }
}
