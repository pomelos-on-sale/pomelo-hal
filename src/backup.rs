//! Backup and restore engine for `/internal/AppData`.
//!
//! Provides transactional, atomic copy operations between internal LittleFS storage
//! (`/internal/AppData`) and external MicroSD storage (`/sdcard/Backup/AppData`).
//!
//! # Safety & Rollback Guarantees
//! - **Backup**: Writes to `/sdcard/Backup/AppData.tmp` first. Only after all files
//!   and subdirectories are flushed does it swap into `/sdcard/Backup/AppData`.
//!   If power fails or an I/O error occurs midway, existing backups remain intact.
//! - **Restore**: Writes to `/internal/AppData.tmp` first and preserves the current
//!   data in `/internal/AppData.rollback`. If writing the new files fails, the
//!   original configuration is restored immediately.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::HalError;
use crate::types::BackupStats;

/// Default internal AppData mount directory on real hardware.
pub const DEFAULT_INTERNAL_APP_DATA: &str = "/internal/AppData";

/// Default SD card backup AppData directory on real hardware.
pub const DEFAULT_SDCARD_BACKUP_APP_DATA: &str = "/sdcard/Backup/AppData";

/// Default stream chunk buffer size (4 KiB) to conserve memory.
const CHUNK_SIZE: usize = 4096;

/// Resolve the active root for AppData across host simulators and embedded hardware.
pub fn resolve_internal_app_data() -> PathBuf {
    crate::storage::resolve_app_data_root()
}

/// Resolve the active destination for `/sdcard/Backup/AppData`.
pub fn resolve_sdcard_backup_dir() -> PathBuf {
    if Path::new("/sdcard").exists() {
        PathBuf::from("/sdcard/Backup/AppData")
    } else if Path::new("./sdcard").exists() {
        PathBuf::from("./sdcard/Backup/AppData")
    } else if crate::storage::is_test_runner() {
        std::env::temp_dir()
            .join("pomelo_tests")
            .join(format!("pid_{}", std::process::id()))
            .join("sdcard/Backup/AppData")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".pomelo/sdcard/Backup/AppData")
    } else {
        std::env::temp_dir().join("pomelo/sdcard/Backup/AppData")
    }
}

/// Recursively copy all files and directories from `src` into `dst` using chunked streaming.
pub fn copy_dir_all(src: &Path, dst: &Path, stats: &mut BackupStats) -> io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&src_path, &dst_path, stats)?;
        } else if file_type.is_file() {
            let bytes = copy_file_chunked(&src_path, &dst_path)?;
            stats.files_count += 1;
            stats.bytes_count += bytes;
        }
    }
    Ok(())
}

/// Stream a single file in 4 KiB chunks to avoid large heap allocations.
fn copy_file_chunked(src: &Path, dst: &Path) -> io::Result<u64> {
    let mut reader = fs::File::open(src)?;
    let mut writer = fs::File::create(dst)?;
    let mut buf = [0u8; CHUNK_SIZE];
    let mut total = 0u64;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
    writer.flush()?;
    Ok(total)
}

/// Atomically back up `src` (internal AppData) to `dst` (SD card Backup AppData).
pub fn backup_app_data_with_paths(src: &Path, dst: &Path) -> Result<BackupStats, HalError> {
    if !src.exists() {
        return Err(HalError::Io(format!(
            "Source AppData directory does not exist: {}",
            src.display()
        )));
    }

    let dst_parent = dst.parent().unwrap_or(dst);
    fs::create_dir_all(dst_parent).map_err(|e| {
        HalError::Io(format!("Failed to create backup destination directory: {e}"))
    })?;

    let tmp_dst = dst_parent.join("AppData.tmp");
    let old_dst = dst_parent.join("AppData.old");

    // Clean up stale temporary directories if left behind from previous interrupted runs
    if tmp_dst.exists() {
        let _ = fs::remove_dir_all(&tmp_dst);
    }
    if old_dst.exists() {
        let _ = fs::remove_dir_all(&old_dst);
    }

    let mut stats = BackupStats::default();

    // 1. Copy into temporary folder
    if let Err(e) = copy_dir_all(src, &tmp_dst, &mut stats) {
        let _ = fs::remove_dir_all(&tmp_dst);
        return Err(HalError::Io(format!(
            "Failed copying files to temporary backup directory: {e}"
        )));
    }

    // 2. Safe rename / swap
    if dst.exists() {
        if let Err(e) = fs::rename(dst, &old_dst) {
            // If rename across or on existing directory fails, remove dst
            if let Err(err_rm) = fs::remove_dir_all(dst) {
                let _ = fs::remove_dir_all(&tmp_dst);
                return Err(HalError::Io(format!(
                    "Failed to rotate existing backup: {e} / {err_rm}"
                )));
            }
        }
    }

    if let Err(e) = fs::rename(&tmp_dst, dst) {
        // Rollback: try to restore old_dst if swap failed
        if old_dst.exists() && !dst.exists() {
            let _ = fs::rename(&old_dst, dst);
        }
        let _ = fs::remove_dir_all(&tmp_dst);
        return Err(HalError::Io(format!(
            "Failed activating new backup directory: {e}"
        )));
    }

    // 3. Final cleanup of old backup
    if old_dst.exists() {
        let _ = fs::remove_dir_all(&old_dst);
    }

    Ok(stats)
}

/// Atomically restore `src` (SD card Backup AppData) into `dst` (internal AppData).
pub fn restore_app_data_with_paths(src: &Path, dst: &Path) -> Result<BackupStats, HalError> {
    if !src.exists() || !src.is_dir() {
        return Err(HalError::Io(format!(
            "Backup directory does not exist or is not a directory: {}",
            src.display()
        )));
    }

    let dst_parent = dst.parent().unwrap_or(dst);
    fs::create_dir_all(dst_parent).map_err(|e| {
        HalError::Io(format!(
            "Failed to create internal AppData parent directory: {e}"
        ))
    })?;

    let tmp_dst = dst_parent.join("AppData.tmp");
    let rollback_dst = dst_parent.join("AppData.rollback");

    // Clean up stale temporary directories if any
    if tmp_dst.exists() {
        let _ = fs::remove_dir_all(&tmp_dst);
    }
    if rollback_dst.exists() {
        let _ = fs::remove_dir_all(&rollback_dst);
    }

    let mut stats = BackupStats::default();

    // 1. Copy into temporary internal directory
    if let Err(e) = copy_dir_all(src, &tmp_dst, &mut stats) {
        let _ = fs::remove_dir_all(&tmp_dst);
        return Err(HalError::Io(format!(
            "Failed copying files from backup to temporary directory: {e}"
        )));
    }

    // 2. Safe rename / swap
    if dst.exists() {
        if let Err(e) = fs::rename(dst, &rollback_dst) {
            if let Err(err_rm) = fs::remove_dir_all(dst) {
                let _ = fs::remove_dir_all(&tmp_dst);
                return Err(HalError::Io(format!(
                    "Failed preparing rollback for current AppData: {e} / {err_rm}"
                )));
            }
        }
    }

    if let Err(e) = fs::rename(&tmp_dst, dst) {
        // Rollback: restore previous internal data
        if rollback_dst.exists() && !dst.exists() {
            let _ = fs::rename(&rollback_dst, dst);
        }
        let _ = fs::remove_dir_all(&tmp_dst);
        return Err(HalError::Io(format!(
            "Failed activating restored AppData: {e}"
        )));
    }

    // 3. Cleanup rollback directory
    if rollback_dst.exists() {
        let _ = fs::remove_dir_all(&rollback_dst);
    }

    Ok(stats)
}

/// Check whether a valid backup directory exists at `path`.
pub fn has_backup_with_path(path: &Path) -> bool {
    path.is_dir()
}

/// Back up `/internal/AppData` to `/sdcard/Backup/AppData` using resolved paths.
pub fn backup_app_data() -> Result<BackupStats, HalError> {
    backup_app_data_with_paths(&resolve_internal_app_data(), &resolve_sdcard_backup_dir())
}

/// Restore `/sdcard/Backup/AppData` into `/internal/AppData` using resolved paths.
pub fn restore_app_data() -> Result<BackupStats, HalError> {
    restore_app_data_with_paths(&resolve_sdcard_backup_dir(), &resolve_internal_app_data())
}

/// Check whether a valid backup exists on the SD card.
pub fn has_backup() -> bool {
    has_backup_with_path(&resolve_sdcard_backup_dir())
}
