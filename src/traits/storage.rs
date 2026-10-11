//! Storage / SD card interface.

use crate::error::HalError;
use crate::types::{SdCardInfo, SdCardStatus, StorageStats};

/// Storage and removable media management (SD/TF card slot).
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage subsystem.
    fn init(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    /// Current status of the SD card.
    fn sd_status(&self) -> SdCardStatus;

    /// Retrieve hardware identity and metadata of the physical SD card (if present).
    fn sd_card_info(&self) -> Option<SdCardInfo> {
        None
    }

    /// Mount the SD card filesystem (typically at `/sdcard`).
    fn mount_sd(&mut self) -> Result<(), HalError>;

    /// Unmount the SD card filesystem.
    fn unmount_sd(&mut self) -> Result<(), HalError>;

    /// Format the physical SD card with a FAT32 filesystem and mount it.
    fn format_sd(&mut self) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    /// Query storage capacity and usage stats of the mounted SD card.
    fn sd_stats(&self) -> Result<StorageStats, HalError>;

    /// Check for hot-plug or state transitions. Called periodically by [`crate::Board::tick`].
    fn tick(&mut self) {}
}
