//! # pomelo-hal
//!
//! The hardware **interface** for the Pomelo OS board, plus the desktop simulator that
//! implements it.
//!
//! The crate names no hardware. It holds the traits every platform implements, the shared
//! types and errors, the [`Board`] facade that groups them, and a pure-Rust simulator for
//! developing on a host. The ESP32-S3 implementation is **not** here: it lives in
//! `firmware/pomelo-hal-esp32`, next to the C drivers it declares, and the composition root
//! (`rust_main`) builds it and injects the resulting board into the apps.
//!
//! Layers, one file per hardware domain (`power`, `wifi`, `audio`, `mic`, `imu`):
//!
//! * [`traits`] — the hardware **interface** every platform must implement.
//! * [`sim`]    — the desktop **simulation** backends (non-`espidf` platforms only).
//! * `firmware/pomelo-hal-esp32` — the real hardware, deliberately outside this crate.
//!
//! Application code should depend only on the [`Board`] facade and the traits, never on
//! `extern "C"` declarations directly. See
//! `issues-and-todo/zh/260926-03-hardware-interfaces-architecture.md` for the full
//! architecture and phased roadmap.
//!
//! On the name: "HAL" here means *board-level services* — the peripherals this board has —
//! not the register-level HAL that `esp-hal` means. Nothing in here talks to a register.

pub mod audio_decoder;
pub mod backup;
pub mod board;
pub mod error;
pub mod storage;
pub mod traits;
pub mod types;
pub mod wakeup;
pub mod wifi_credentials;

/// Desktop simulator backends, on every platform that is not the device itself.
#[cfg(not(target_os = "espidf"))]
pub mod sim;

pub use audio_decoder::{format_time, probe_audio_meta, AudioDecoder};
pub use backup::{backup_app_data, has_backup, restore_app_data};
pub use board::Board;
pub use error::HalError;
pub use storage::{
    kv_clear_all, kv_delete_raw, kv_dump_all, kv_get_raw, kv_load, kv_save, kv_set_raw,
    PersistentConfig,
};
pub use traits::{
    AudioBackend, HttpBackend, ImuBackend, InputBackend, MicBackend, PowerBackend, StorageBackend,
    TimeBackend, WifiBackend, VALID_TIME_EPOCH_2026,
};
pub use types::{
    ApInfo, AudioMeta, BackupStats, HttpState, InputAction, ScanState, SdCardInfo, SdCardStatus,
    StorageStats, SystemEvent, Vec3, WakeupReason, WifiState, WifiStatus,
};
pub use wakeup::{WakeupRequest, WakeupScheduler};
pub use wifi_credentials::WifiCredentials;

/// Convenience import for application crates.
pub mod prelude {
    pub use crate::board::Board;
    pub use crate::error::HalError;
    pub use crate::storage::PersistentConfig;
    pub use crate::traits::{
        AudioBackend, HttpBackend, ImuBackend, InputBackend, MicBackend, PowerBackend,
        StorageBackend, TimeBackend, WifiBackend,
    };
    pub use crate::types::{
        ApInfo, AudioMeta, HttpState, InputAction, ScanState, SdCardInfo, SdCardStatus,
        StorageStats, SystemEvent, Vec3, WifiState, WifiStatus,
    };
    pub use crate::wifi_credentials::WifiCredentials;
}

