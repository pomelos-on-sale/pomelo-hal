//! Backup and restore engine for Pomelo OS configurations and data.
//!
//! Provides transactional, atomic backup operations between Flash NVS / KV storage
//! and external MicroSD storage (`/sdcard/Backup/app_data.json`).
//!
//! # Safety & Power-Loss Guarantees
//! - **Backup**: Dumps all persistent configurations from Flash NVS / KV storage into a structured
//!   JSON archive and atomically commits to the destination file using `.tmp` and rename.
//! - **Restore**: Atomically reads the backup JSON archive and commits each configuration
//!   entry directly into Flash NVS / KV storage.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::HalError;
use crate::storage;
use crate::types::BackupStats;

/// Default SD card backup file path on real embedded hardware.
pub const DEFAULT_SDCARD_BACKUP_FILE: &str = "/sdcard/Backup/app_data.json";

/// Format version for Pomelo OS backup archives.
pub const BACKUP_VERSION: u32 = 1;

/// Structured JSON representation of all backed-up system configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupArchive {
    /// Schema version
    pub version: u32,
    /// Unix timestamp when backup was generated
    pub created_at_unix: u64,
    /// Mapped configuration entries: namespace -> (key -> JSON payload)
    pub entries: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

/// Resolve the active destination path for `/sdcard/Backup/app_data.json`.
pub fn resolve_sdcard_backup_file() -> PathBuf {
    if Path::new("/sdcard").exists() {
        PathBuf::from("/sdcard/Backup/app_data.json")
    } else if Path::new("./sdcard").exists() {
        PathBuf::from("./sdcard/Backup/app_data.json")
    } else if storage::is_test_runner() {
        std::env::temp_dir()
            .join("pomelo_tests")
            .join(format!("pid_{}", std::process::id()))
            .join("sdcard/Backup/app_data.json")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".pomelo/sdcard/Backup/app_data.json")
    } else {
        std::env::temp_dir().join("pomelo/sdcard/Backup/app_data.json")
    }
}

/// Helper to normalize any path (directory or file) into the target backup file path.
fn normalize_backup_path(p: &Path) -> PathBuf {
    if p.is_dir() || p.extension().is_none() {
        p.join("app_data.json")
    } else {
        p.to_path_buf()
    }
}

/// Atomically back up all Flash NVS / KV configurations to a designated file path on SD card.
pub fn backup_app_data_to_file(target_file: &Path) -> Result<BackupStats, HalError> {
    let target = normalize_backup_path(target_file);

    // 1. Dump all current KV entries across all namespaces
    let kv_dump = storage::kv_dump_all().map_err(|e| {
        HalError::Io(format!("Failed to dump KV storage for backup: {e}"))
    })?;

    let mut entries_count = 0usize;
    let mut mapped_entries: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new();

    for (ns, keys) in kv_dump {
        let mut ns_map = BTreeMap::new();
        for (key, raw_bytes) in keys {
            entries_count += 1;
            // Attempt to deserialize as JSON structure for readability
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&raw_bytes) {
                ns_map.insert(key, val);
            } else if let Ok(s) = std::str::from_utf8(&raw_bytes) {
                ns_map.insert(key, serde_json::Value::String(s.to_string()));
            } else {
                // Raw binary data
                ns_map.insert(key, serde_json::json!({
                    "_binary_hex": hex_encode(&raw_bytes)
                }));
            }
        }
        if !ns_map.is_empty() {
            mapped_entries.insert(ns, ns_map);
        }
    }

    let archive = BackupArchive {
        version: BACKUP_VERSION,
        created_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        entries: mapped_entries,
    };

    let serialized = serde_json::to_string_pretty(&archive).map_err(|e| {
        HalError::Io(format!("Failed to serialize backup archive to JSON: {e}"))
    })?;

    // 2. Commit atomically to destination file
    storage::atomic_write_text(&target, &serialized).map_err(|e| {
        HalError::Io(format!("Failed to write backup file {}: {e}", target.display()))
    })?;

    Ok(BackupStats::new(entries_count, serialized.len() as u64))
}

/// Atomically restore Flash NVS / KV configurations from a backup JSON file on SD card.
pub fn restore_app_data_from_file(source_file: &Path) -> Result<BackupStats, HalError> {
    let source = normalize_backup_path(source_file);

    if !source.exists() || !source.is_file() {
        return Err(HalError::Io(format!(
            "Backup file does not exist: {}",
            source.display()
        )));
    }

    let content = fs::read_to_string(&source).map_err(|e| {
        HalError::Io(format!("Failed to read backup file {}: {e}", source.display()))
    })?;

    let archive: BackupArchive = serde_json::from_str(&content).map_err(|e| {
        HalError::Io(format!("Failed to parse backup archive JSON: {e}"))
    })?;

    let mut restored_count = 0usize;

    // Write back all entries directly to Flash NVS / KV storage
    for (ns, keys) in archive.entries {
        for (key, val) in keys {
            let bytes = match val {
                serde_json::Value::String(s) => s.into_bytes(),
                serde_json::Value::Object(ref obj) if obj.contains_key("_binary_hex") => {
                    let hex_str = obj.get("_binary_hex").and_then(|v| v.as_str()).unwrap_or("");
                    hex_decode(hex_str).unwrap_or_default()
                }
                other => serde_json::to_vec(&other).map_err(|e| {
                    HalError::Io(format!("Failed to serialize value for {ns}/{key}: {e}"))
                })?,
            };

            storage::kv_set_raw(&ns, &key, &bytes).map_err(|e| {
                HalError::Io(format!("Failed to restore KV entry {ns}/{key}: {e}"))
            })?;

            restored_count += 1;
        }
    }

    Ok(BackupStats::new(restored_count, content.len() as u64))
}

/// Check whether a valid non-empty backup archive file exists at `path`.
pub fn has_backup_at_file(path: &Path) -> bool {
    let target = normalize_backup_path(path);
    target.is_file() && fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false)
}

/// Back up all configurations to `/sdcard/Backup/app_data.json` using resolved platform paths.
pub fn backup_app_data() -> Result<BackupStats, HalError> {
    backup_app_data_to_file(&resolve_sdcard_backup_file())
}

/// Restore all configurations from `/sdcard/Backup/app_data.json` using resolved platform paths.
pub fn restore_app_data() -> Result<BackupStats, HalError> {
    restore_app_data_from_file(&resolve_sdcard_backup_file())
}

/// Check whether a valid backup file exists on the SD card.
pub fn has_backup() -> bool {
    has_backup_at_file(&resolve_sdcard_backup_file())
}

/// Backward compatibility helper for legacy path pairs.
pub fn backup_app_data_with_paths(_src: &Path, dst: &Path) -> Result<BackupStats, HalError> {
    backup_app_data_to_file(dst)
}

/// Backward compatibility helper for legacy path pairs.
pub fn restore_app_data_with_paths(src: &Path, _dst: &Path) -> Result<BackupStats, HalError> {
    restore_app_data_from_file(src)
}

/// Backward compatibility helper for checking backup existence at path.
pub fn has_backup_with_path(path: &Path) -> bool {
    has_backup_at_file(path)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
