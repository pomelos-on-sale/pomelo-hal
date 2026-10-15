//! Application configuration and persistent storage infrastructure.
//!
//! Provides a unified, safe, and robust foundation for all Pomelo OS applications
//! and system services to persist configurations and data to `/internal/AppData` on
//! real embedded hardware, or `~/.pomelo/AppData` / `POMELO_APP_DATA` on desktop hosts.
//!
//! # Core Guarantees
//! - **Power-Loss Atomicity**: Uses atomic write-and-rename (`.tmp` -> target file).
//!   A sudden power cutoff will never corrupt or leave a half-written configuration.
//! - **Flash Wear Mitigation**: Performs content deduplication. If the serialized
//!   configuration matches what is already on disk, the Flash sector erase/write is skipped.
//! - **Type-Safe Schema**: The [`PersistentConfig`] trait integrates `serde` and `toml`,
//!   providing ergonomic, boilerplate-free `load()` and `save()`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Default internal AppData directory on real embedded hardware (SPIFFS / LittleFS).
pub const BOARD_APP_DATA: &str = "/internal/AppData";

/// Default desktop AppData directory under user home folder.
pub const DESKTOP_APP_DATA_DIR: &str = ".pomelo/AppData";

/// Returns whether the current process is running as a test harness binary.
pub fn is_test_runner() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        if let Ok(exe) = std::env::current_exe() {
            let s = exe.to_string_lossy();
            if s.contains("/target/") && s.contains("/deps/") {
                return true;
            }
        }
        false
    }
}

/// Resolves the active root directory for application data across embedded boards,
/// desktop simulator, and isolated testing environments.
pub fn resolve_app_data_root() -> PathBuf {
    // 1. Environment variable override (crucial for unit tests and multi-instance testing)
    if let Ok(custom) = std::env::var("POMELO_APP_DATA") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    // 2. Embedded hardware partitions or local project internal directories
    if Path::new("/internal/AppData").exists() || Path::new("/internal").exists() {
        PathBuf::from("/internal/AppData")
    } else if Path::new("./internal/AppData").exists() || Path::new("./internal").exists() {
        PathBuf::from("./internal/AppData")
    } else if is_test_runner() {
        // In automated tests, use process-isolated temp directory to avoid host pollution
        std::env::temp_dir()
            .join("pomelo_tests")
            .join(format!("pid_{}", std::process::id()))
    } else if let Ok(home) = std::env::var("HOME") {
        // Desktop user home directory (~/.pomelo/AppData)
        PathBuf::from(home).join(DESKTOP_APP_DATA_DIR)
    } else {
        std::env::temp_dir().join("pomelo/AppData")
    }
}

/// Resolves the storage directory for a specific application or subsystem (e.g. "SETTINGS", "WIFI").
///
/// Ensures the directory exists before returning.
pub fn app_dir(app_id: &str) -> PathBuf {
    let dir = resolve_app_data_root().join(app_id);
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Resolves the full path to a named file for an application.
pub fn app_file_path(app_id: &str, file_name: &str) -> PathBuf {
    app_dir(app_id).join(file_name)
}

/// Atomically writes bytes to a file by writing to a temporary file first,
/// flushing to disk, and renaming to the target path.
///
/// If `skip_if_unchanged` is true and the target file already exists with identical
/// bytes, the write is completely skipped to prevent unnecessary Flash wear.
pub fn atomic_write(target_path: impl AsRef<Path>, data: &[u8], skip_if_unchanged: bool) -> io::Result<()> {
    let target = target_path.as_ref();

    // 1. Flash wear protection: skip write if content is byte-for-byte identical
    if skip_if_unchanged && target.exists() {
        if let Ok(existing) = fs::read(target) {
            if existing == data {
                return Ok(());
            }
        }
    }

    // 2. Ensure parent directory exists
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    // 3. Create temporary file in the same directory (required for atomic rename across filesystems)
    let temp_name = format!(
        ".{}.tmp.{}",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    );
    let temp_path = target.with_file_name(temp_name);

    // 4. Write data and flush to ensure media commitment
    {
        let mut file = File::create(&temp_path)?;
        file.write_all(data)?;
        file.flush()?;
        let _ = file.sync_all();
    }

    // 5. Atomic rename to target path
    if let Err(e) = fs::rename(&temp_path, target) {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    Ok(())
}

/// Atomically writes a UTF-8 text string to a target file with wear protection.
pub fn atomic_write_text(target_path: impl AsRef<Path>, text: &str) -> io::Result<()> {
    atomic_write(target_path, text.as_bytes(), true)
}

/// Loads and deserializes a TOML file from disk.
pub fn load_toml<T: DeserializeOwned>(path: impl AsRef<Path>) -> io::Result<T> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serializes and atomically writes a value to a TOML file.
pub fn save_toml_atomic<T: Serialize>(path: impl AsRef<Path>, value: &T) -> io::Result<()> {
    let content = toml::to_string_pretty(value)
        .or_else(|_| toml::to_string(value))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write_text(path, &content)
}

/// A standard trait for persistent configuration structs.
///
/// Types implementing this trait automatically gain type-safe, atomic,
/// and wear-mitigated `load()` and `save()` operations.
pub trait PersistentConfig: Serialize + DeserializeOwned + Default {
    /// Directory name under AppData (e.g. "SETTINGS", "DAILY_SCHEDULE", "WIFI").
    const APP_ID: &'static str;

    /// File name within the application directory (defaults to "config.toml").
    const FILE_NAME: &'static str = "config.toml";

    /// Returns the full path to the configuration file.
    fn config_path() -> PathBuf {
        app_file_path(Self::APP_ID, Self::FILE_NAME)
    }

    /// Loads the configuration from disk, falling back to `Default::default()`
    /// if the file does not exist or contains invalid data.
    fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    /// Attempts to load the configuration, returning an error if the file
    /// cannot be read or parsed.
    fn try_load() -> io::Result<Self> {
        load_toml(Self::config_path())
    }

    /// Saves the configuration atomically to disk.
    fn save(&self) -> io::Result<()> {
        save_toml_atomic(Self::config_path(), self)
    }

    /// Removes the configuration file from disk.
    fn forget() -> io::Result<()> {
        let path = Self::config_path();
        if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use serde::{Deserialize, Serialize};

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn with_test_dir<F: FnOnce(&Path)>(f: F) {
        let _guard = TEST_MUTEX.lock().unwrap();
        let test_root = std::env::temp_dir().join(format!("pomelo_storage_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&test_root);
        fs::create_dir_all(&test_root).unwrap();

        std::env::set_var("POMELO_APP_DATA", &test_root);
        f(&test_root);
        std::env::remove_var("POMELO_APP_DATA");
        let _ = fs::remove_dir_all(&test_root);
    }

    #[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
    struct SampleConfig {
        pub count: u32,
        pub name: String,
        pub enabled: bool,
    }

    impl PersistentConfig for SampleConfig {
        const APP_ID: &'static str = "SAMPLE_APP";
        const FILE_NAME: &'static str = "sample.toml";
    }

    #[test]
    fn persistent_config_lifecycle_load_save_forget() {
        with_test_dir(|_root| {
            // 1. Missing file returns default
            let initial = SampleConfig::load();
            assert_eq!(initial, SampleConfig::default());

            // 2. Modify and save atomically
            let cfg = SampleConfig {
                count: 42,
                name: "pomelo".into(),
                enabled: true,
            };
            cfg.save().expect("save should succeed");

            // 3. Reload from disk
            let loaded = SampleConfig::load();
            assert_eq!(loaded, cfg);

            // 4. Forget file
            SampleConfig::forget().expect("forget should succeed");
            let after_forget = SampleConfig::load();
            assert_eq!(after_forget, SampleConfig::default());
        });
    }

    #[test]
    fn atomic_write_wear_protection_skips_identical_content() {
        with_test_dir(|root| {
            let file_path = root.join("test_wear.txt");
            atomic_write_text(&file_path, "hello world").unwrap();

            // Metadata check
            let mtime1 = fs::metadata(&file_path).unwrap().modified().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Write identical text with skip_if_unchanged = true
            atomic_write_text(&file_path, "hello world").unwrap();
            let mtime2 = fs::metadata(&file_path).unwrap().modified().unwrap();

            assert_eq!(mtime1, mtime2, "identical write must be skipped to protect Flash");

            // Write changed text
            atomic_write_text(&file_path, "hello changed").unwrap();
            let mtime3 = fs::metadata(&file_path).unwrap().modified().unwrap();
            assert_ne!(mtime1, mtime3, "changed write must proceed");
            assert_eq!(fs::read_to_string(&file_path).unwrap(), "hello changed");
        });
    }

    #[test]
    fn corrupted_file_falls_back_to_default_gracefully() {
        with_test_dir(|_root| {
            let path = SampleConfig::config_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "invalid [[[ toml syntax").unwrap();

            assert!(SampleConfig::try_load().is_err());
            assert_eq!(SampleConfig::load(), SampleConfig::default());
        });
    }
}
