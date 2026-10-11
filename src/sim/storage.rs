//! Desktop simulation backend for storage and SD card.

use std::path::{Path, PathBuf};

use crate::error::HalError;
use crate::traits::StorageBackend;
use crate::types::{SdCardInfo, SdCardStatus, StorageStats};

/// A simulated storage backend that maps an SD card to a local host directory.
pub struct SimStorage {
    status: SdCardStatus,
    mount_dir: PathBuf,
}

impl SimStorage {
    /// Create a simulated storage backend pointed at `./sdcard`.
    pub fn new() -> Self {
        Self::with_path("sdcard")
    }

    /// Create a simulated storage backend with a custom path.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        let mount_dir = path.into();
        let status = if mount_dir.exists() {
            if mount_dir.join(".unformatted").exists() {
                SdCardStatus::Unformatted
            } else {
                SdCardStatus::Mounted
            }
        } else {
            SdCardStatus::NotInserted
        };
        Self { status, mount_dir }
    }

    /// Path to the simulated SD card directory on the host filesystem.
    pub fn mount_path(&self) -> &Path {
        &self.mount_dir
    }
}

impl Default for SimStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for SimStorage {
    fn sd_status(&self) -> SdCardStatus {
        self.status
    }

    fn sd_card_info(&self) -> Option<SdCardInfo> {
        if self.status == SdCardStatus::Mounted || self.status == SdCardStatus::Unformatted {
            Some(SdCardInfo {
                name: "SIM_SD_32G".to_string(),
                total_bytes: 32 * 1024 * 1024 * 1024,
                serial_number: 0x87654321,
                is_formatted: self.status == SdCardStatus::Mounted,
            })
        } else {
            None
        }
    }

    fn mount_sd(&mut self) -> Result<(), HalError> {
        if self.status == SdCardStatus::Mounted {
            return Ok(());
        }
        if !self.mount_dir.exists() {
            let _ = std::fs::create_dir_all(&self.mount_dir);
        }
        if self.mount_dir.join(".unformatted").exists() {
            self.status = SdCardStatus::Unformatted;
        } else {
            self.status = SdCardStatus::Mounted;
        }
        Ok(())
    }

    fn unmount_sd(&mut self) -> Result<(), HalError> {
        self.status = SdCardStatus::NotInserted;
        Ok(())
    }

    fn format_sd(&mut self) -> Result<(), HalError> {
        if !self.mount_dir.exists() {
            let _ = std::fs::create_dir_all(&self.mount_dir);
        }
        let unformatted_marker = self.mount_dir.join(".unformatted");
        if unformatted_marker.exists() {
            let _ = std::fs::remove_file(unformatted_marker);
        }
        self.status = SdCardStatus::Mounted;
        Ok(())
    }

    fn sd_stats(&self) -> Result<StorageStats, HalError> {
        if self.status != SdCardStatus::Mounted {
            return Err(HalError::NotInitialized);
        }
        // Simulated: 32 GB SD card with ~29.8 GB free
        Ok(StorageStats::new(
            32 * 1024 * 1024 * 1024,
            29 * 1024 * 1024 * 1024 + 800 * 1024 * 1024,
        ))
    }

    fn tick(&mut self) {
        if self.mount_dir.exists() {
            if self.mount_dir.join(".unformatted").exists() {
                self.status = SdCardStatus::Unformatted;
            } else {
                self.status = SdCardStatus::Mounted;
            }
        } else {
            self.status = SdCardStatus::NotInserted;
        }
    }
}
