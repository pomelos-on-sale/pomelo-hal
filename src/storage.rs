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
//! - **Type-Safe Schema**: The [`PersistentConfig`] trait integrates `serde` and `serde_json`,
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

/// Loads and deserializes a JSON file from disk.
pub fn load_json<T: DeserializeOwned>(path: impl AsRef<Path>) -> io::Result<T> {
    let content = fs::read(path)?;
    serde_json::from_slice(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serializes and atomically writes a value to a JSON file.
pub fn save_json_atomic<T: Serialize>(path: impl AsRef<Path>, value: &T) -> io::Result<()> {
    let content = serde_json::to_vec_pretty(value)
        .or_else(|_| serde_json::to_vec(value))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    atomic_write(path, &content, true)
}

#[cfg(target_os = "espidf")]
mod nvs_ffi {
    pub type HalNvsDumpCb = unsafe extern "C" fn(
        ns: *const std::os::raw::c_char,
        key: *const std::os::raw::c_char,
        data: *const u8,
        len: usize,
        ctx: *mut std::ffi::c_void,
    );

    extern "C" {
        pub fn hal_nvs_get_blob(
            part: *const std::os::raw::c_char,
            ns: *const std::os::raw::c_char,
            key: *const std::os::raw::c_char,
            out_buf: *mut u8,
            inout_len: *mut usize,
        ) -> i32;
        pub fn hal_nvs_set_blob(
            part: *const std::os::raw::c_char,
            ns: *const std::os::raw::c_char,
            key: *const std::os::raw::c_char,
            data: *const u8,
            len: usize,
        ) -> i32;
        pub fn hal_nvs_erase_key(
            part: *const std::os::raw::c_char,
            ns: *const std::os::raw::c_char,
            key: *const std::os::raw::c_char,
        ) -> i32;
        pub fn hal_nvs_dump_all(
            part: *const std::os::raw::c_char,
            cb: HalNvsDumpCb,
            ctx: *mut std::ffi::c_void,
        ) -> i32;
    }
}

/// Raw low-level KV get: retrieves binary data from Flash NVS (on ESP32)
/// or from mock persistent file storage (on desktop simulator/hosts).
pub fn kv_get_raw(namespace: &str, key: &str) -> io::Result<Option<Vec<u8>>> {
    #[cfg(target_os = "espidf")]
    {
        use std::ffi::CString;
        let c_part = CString::new("nvs_app").unwrap();
        let c_ns = CString::new(namespace).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let c_key = CString::new(key).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let mut len: usize = 0;
        let ret = unsafe {
            nvs_ffi::hal_nvs_get_blob(
                c_part.as_ptr(),
                c_ns.as_ptr(),
                c_key.as_ptr(),
                std::ptr::null_mut(),
                &mut len,
            )
        };
        // ESP_ERR_NVS_NOT_FOUND (0x1102) or ESP_ERR_NVS_NOT_INITIALIZED (0x1101)
        if ret == 0x1102 || ret == 0x1101 {
            return Ok(None);
        }
        if ret != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, format!("nvs_get_blob query error: {ret:#x}")));
        }

        let mut buf = vec![0u8; len];
        let ret = unsafe {
            nvs_ffi::hal_nvs_get_blob(
                c_part.as_ptr(),
                c_ns.as_ptr(),
                c_key.as_ptr(),
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if ret == 0 {
            buf.truncate(len);
            Ok(Some(buf))
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("nvs_get_blob read error: {ret:#x}")))
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let p = kv_mock_file(namespace, key);
        if p.exists() {
            fs::read(&p).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// Raw low-level KV set: saves binary data to Flash NVS (on ESP32)
/// or to mock persistent file storage (on desktop simulator/hosts).
pub fn kv_set_raw(namespace: &str, key: &str, data: &[u8]) -> io::Result<()> {
    #[cfg(target_os = "espidf")]
    {
        use std::ffi::CString;
        let c_part = CString::new("nvs_app").unwrap();
        let c_ns = CString::new(namespace).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let c_key = CString::new(key).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let ret = unsafe {
            nvs_ffi::hal_nvs_set_blob(
                c_part.as_ptr(),
                c_ns.as_ptr(),
                c_key.as_ptr(),
                data.as_ptr(),
                data.len(),
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("nvs_set_blob error: {ret:#x}")))
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let p = kv_mock_file(namespace, key);
        atomic_write(p, data, true)
    }
}

/// Raw low-level KV delete: erases key from Flash NVS (on ESP32)
/// or from mock persistent file storage (on desktop simulator/hosts).
pub fn kv_delete_raw(namespace: &str, key: &str) -> io::Result<()> {
    #[cfg(target_os = "espidf")]
    {
        use std::ffi::CString;
        let c_part = CString::new("nvs_app").unwrap();
        let c_ns = CString::new(namespace).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let c_key = CString::new(key).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let ret = unsafe {
            nvs_ffi::hal_nvs_erase_key(
                c_part.as_ptr(),
                c_ns.as_ptr(),
                c_key.as_ptr(),
            )
        };
        if ret == 0 || ret == 0x1102 {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("nvs_erase_key error: {ret:#x}")))
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let p = kv_mock_file(namespace, key);
        if p.exists() {
            let _ = fs::remove_file(&p);
        }
        Ok(())
    }
}

#[cfg(not(target_os = "espidf"))]
fn kv_mock_file(namespace: &str, key: &str) -> PathBuf {
    resolve_app_data_root()
        .join("kv")
        .join(namespace)
        .join(format!("{key}.bin"))
}

/// Loads and deserializes a typed configuration from KV storage.
pub fn kv_load<T: DeserializeOwned>(namespace: &str, key: &str) -> io::Result<T> {
    let bytes = match kv_get_raw(namespace, key)? {
        Some(b) => b,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("key '{namespace}/{key}' not found"),
            ))
        }
    };

    serde_json::from_slice::<T>(&bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("deserialization failed for {namespace}/{key}: {e}"),
        )
    })
}

/// Serializes and saves a typed configuration to KV storage.
pub fn kv_save<T: Serialize>(namespace: &str, key: &str, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    kv_set_raw(namespace, key, &bytes)
}

/// Dumps all key-value entries across all namespaces into an in-memory map.
pub fn kv_dump_all() -> io::Result<std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<u8>>>> {
    let mut result: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<u8>>> = std::collections::BTreeMap::new();

    #[cfg(target_os = "espidf")]
    {
        use std::ffi::{CStr, CString};
        use std::os::raw::c_char;

        unsafe extern "C" fn dump_callback(
            ns: *const c_char,
            key: *const c_char,
            data: *const u8,
            len: usize,
            ctx: *mut std::ffi::c_void,
        ) {
            if ns.is_null() || key.is_null() || data.is_null() || ctx.is_null() {
                return;
            }
            let map = &mut *(ctx as *mut std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<u8>>>);
            let ns_str = CStr::from_ptr(ns).to_string_lossy().to_string();
            let key_str = CStr::from_ptr(key).to_string_lossy().to_string();
            let slice = std::slice::from_raw_parts(data, len);
            map.entry(ns_str).or_default().insert(key_str, slice.to_vec());
        }

        let c_part = CString::new("nvs_app").unwrap();
        unsafe {
            nvs_ffi::hal_nvs_dump_all(
                c_part.as_ptr(),
                dump_callback,
                &mut result as *mut _ as *mut std::ffi::c_void,
            );
        }
    }

    #[cfg(not(target_os = "espidf"))]
    {
        let kv_dir = resolve_app_data_root().join("kv");
        if kv_dir.exists() {
            if let Ok(entries) = fs::read_dir(&kv_dir) {
                for ns_entry in entries.flatten() {
                    if ns_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let ns_name = ns_entry.file_name().to_string_lossy().to_string();
                        if let Ok(files) = fs::read_dir(ns_entry.path()) {
                            for f in files.flatten() {
                                if f.file_type().map(|t| t.is_file()).unwrap_or(false) {
                                    let file_name = f.file_name().to_string_lossy().to_string();
                                    if let Some(key) = file_name.strip_suffix(".bin") {
                                        if let Ok(data) = fs::read(f.path()) {
                                            result.entry(ns_name.clone()).or_default().insert(key.to_string(), data);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Ensure all known system/app namespaces and keys are probed if present
    const KNOWN: &[(&str, &str)] = &[
        ("SETTINGS", "preferences"),
        ("SETTINGS", "config"),
        ("WIFI", "credentials"),
        ("ALARM_CLOCK", "config"),
        ("DAILY_SCHEDULE", "config"),
        ("DAILY_SCHEDULE", "today_cache"),
        ("DAILY_SCHED", "config"),
        ("SYSTEM", "env"),
    ];
    for &(ns, key) in KNOWN {
        let exists = result.get(ns).and_then(|m| m.get(key)).is_some();
        if !exists {
            if let Ok(Some(bytes)) = kv_get_raw(ns, key) {
                result.entry(ns.to_string()).or_default().insert(key.to_string(), bytes);
            }
        }
    }

    Ok(result)
}

/// Clears all key-value entries across all namespaces.
pub fn kv_clear_all() -> io::Result<()> {
    let dump = kv_dump_all()?;
    for (ns, entries) in dump {
        for (key, _) in entries {
            let _ = kv_delete_raw(&ns, &key);
        }
    }
    Ok(())
}


/// A standard trait for persistent configuration structs.
///
/// Types implementing this trait automatically gain type-safe, atomic,
/// and wear-mitigated `load()` and `save()` operations backed by Flash NVS (on board)
/// or mock KV storage (on desktop simulator).
pub trait PersistentConfig: Serialize + DeserializeOwned + Default {
    /// Directory/Namespace under NVS or KV (e.g. "SETTINGS", "DAILY_SCHED", "WIFI").
    /// Keep <= 15 ASCII characters for NVS compatibility.
    const APP_ID: &'static str;

    /// Key under NVS or KV (defaults to "config").
    /// Must be <= 15 ASCII characters for NVS key compatibility.
    const KEY: &'static str = "config";

    /// File name within the application directory for export or fallback.
    const FILE_NAME: &'static str = "config.json";

    /// Returns the full path to the configuration file (export/fallback).
    fn config_path() -> PathBuf {
        app_file_path(Self::APP_ID, Self::FILE_NAME)
    }

    /// Loads the configuration, falling back to `Default::default()`
    /// if the key does not exist or contains invalid data.
    fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    /// Attempts to load the configuration from KV/NVS storage (falling back to file if present).
    fn try_load() -> io::Result<Self> {
        match kv_load(Self::APP_ID, Self::KEY) {
            Ok(val) => Ok(val),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let p = Self::config_path();
                if p.exists() {
                    load_json(&p)
                } else {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Saves the configuration to KV/NVS storage.
    fn save(&self) -> io::Result<()> {
        kv_save(Self::APP_ID, Self::KEY, self)
    }

    /// Removes the configuration from KV/NVS storage.
    fn forget() -> io::Result<()> {
        let _ = kv_delete_raw(Self::APP_ID, Self::KEY);
        let path = Self::config_path();
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        Ok(())
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
        const FILE_NAME: &'static str = "sample.json";
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
            fs::write(&path, "invalid [[[ json syntax").unwrap();

            assert!(SampleConfig::try_load().is_err());
            assert_eq!(SampleConfig::load(), SampleConfig::default());
        });
    }
}
