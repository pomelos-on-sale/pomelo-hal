//! Desktop simulator backends — the HAL **simulation** layer.
//!
//! Compiled on every platform except `target_os = "espidf"`. These provide
//! realistic, deterministic-enough fake data so the whole UI can be developed
//! and unit-tested on a desktop without any hardware attached.
//!
//! One file per hardware domain, mirroring [`crate::traits`]. The other
//! implementation of those traits is the device's, in
//! `firmware/pomelo-hal-esp32/src/`.

mod audio;
mod imu;
mod mic;
mod power;
mod wifi;

pub use audio::SimAudio;
pub use imu::SimImu;
pub use mic::SimMic;
pub use power::SimPower;
pub use wifi::SimWifi;
