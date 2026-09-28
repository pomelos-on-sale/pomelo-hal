//! Simulated QMI8658 IMU with gentle jitter around gravity.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::error::HalError;
use crate::traits::ImuBackend;
use crate::types::Vec3;

pub struct SimImu {
    phase: AtomicU32,
}

impl SimImu {
    pub fn new() -> Self {
        Self {
            phase: AtomicU32::new(0),
        }
    }

    fn next_phase(&self) -> f32 {
        let p = self.phase.fetch_add(1, Ordering::Relaxed) % 628;
        p as f32 / 100.0
    }
}

impl Default for SimImu {
    fn default() -> Self {
        Self::new()
    }
}

impl ImuBackend for SimImu {
    fn read_accel(&self) -> Result<Vec3, HalError> {
        let t = self.next_phase();
        Ok(Vec3::new(
            0.05 * f32::sin(t),
            0.03 * f32::cos(t),
            9.81 + 0.02 * f32::sin(2.0 * t),
        ))
    }

    fn read_gyro(&self) -> Result<Vec3, HalError> {
        let t = self.next_phase();
        Ok(Vec3::new(0.01 * f32::sin(t), 0.01 * f32::cos(t), 0.0))
    }

    fn temperature_c(&self) -> Result<f32, HalError> {
        Ok(31.4)
    }
}
