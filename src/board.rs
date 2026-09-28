//! The [`Board`] facade aggregates every subsystem backend.
//!
//! It names no implementation. Which backends a board has is the *composition root's*
//! decision — `pomelo_hal_esp32::board()` builds this board's hardware on the device,
//! [`Board::simulated`] builds the desktop simulator, and a test may build its own — and
//! the resulting `Arc<Board>` is what application code receives. That is why this crate
//! carries no `extern "C"` and no `target_os` switch: the choice is an argument, not a
//! `#[cfg]`.

use std::sync::{Mutex, MutexGuard};

use crate::traits::{AudioBackend, ImuBackend, MicBackend, PowerBackend, WifiBackend};

/// A shared handle to every hardware subsystem.
///
/// Assemble one with [`Board::from_backends`] (or [`Board::simulated`]) and pass the
/// resulting `Arc<Board>` down into application/UI code. Each backend is guarded by its own
/// mutex, so a lock on one subsystem never blocks another.
pub struct Board {
    power: Mutex<Box<dyn PowerBackend>>,
    wifi: Mutex<Box<dyn WifiBackend>>,
    audio: Mutex<Box<dyn AudioBackend>>,
    mic: Mutex<Box<dyn MicBackend>>,
    imu: Mutex<Box<dyn ImuBackend>>,
}

impl Board {
    /// Assemble a board from one backend per subsystem.
    ///
    /// The order is `power`, `wifi`, `audio`, `mic`, `imu` — the same order the accessors
    /// appear in. It takes boxes rather than generics so the caller can mix concrete types
    /// and, in a test, its own fakes.
    pub fn from_backends(
        power: Box<dyn PowerBackend>,
        wifi: Box<dyn WifiBackend>,
        audio: Box<dyn AudioBackend>,
        mic: Box<dyn MicBackend>,
        imu: Box<dyn ImuBackend>,
    ) -> Self {
        Self {
            power: Mutex::new(power),
            wifi: Mutex::new(wifi),
            audio: Mutex::new(audio),
            mic: Mutex::new(mic),
            imu: Mutex::new(imu),
        }
    }

    /// A board of desktop simulator backends.
    #[cfg(not(target_os = "espidf"))]
    pub fn simulated() -> Self {
        use crate::sim::{SimAudio, SimImu, SimMic, SimPower, SimWifi};
        Self::from_backends(
            Box::new(SimPower::new()),
            Box::new(SimWifi::new()),
            Box::new(SimAudio::new()),
            Box::new(SimMic::new()),
            Box::new(SimImu::new()),
        )
    }

    /// Initialize every backend that requires explicit setup.
    ///
    /// Idempotent and best-effort: errors are ignored so a missing optional
    /// peripheral never aborts boot. Call once at startup.
    pub fn init(&self) {
        let _ = self.power().init();
        let _ = self.wifi().init();
    }

    /// Lock the power/battery backend.
    pub fn power(&self) -> MutexGuard<'_, Box<dyn PowerBackend>> {
        lock(&self.power)
    }

    /// Lock the Wi-Fi backend.
    pub fn wifi(&self) -> MutexGuard<'_, Box<dyn WifiBackend>> {
        lock(&self.wifi)
    }

    /// Lock the audio backend.
    pub fn audio(&self) -> MutexGuard<'_, Box<dyn AudioBackend>> {
        lock(&self.audio)
    }

    /// Lock the microphone backend.
    pub fn mic(&self) -> MutexGuard<'_, Box<dyn MicBackend>> {
        lock(&self.mic)
    }

    /// Lock the IMU backend.
    pub fn imu(&self) -> MutexGuard<'_, Box<dyn ImuBackend>> {
        lock(&self.imu)
    }

    /// Advance every time-driven subsystem (Wi-Fi scan progress, audio EOF
    /// detection). Call once per UI frame.
    pub fn tick(&self) {
        self.wifi().tick();
        self.audio().tick();
    }
}

/// Lock a mutex, recovering the inner value if the lock was poisoned so a
/// panic in one backend never takes down the whole UI.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
