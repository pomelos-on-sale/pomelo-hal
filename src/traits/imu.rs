//! IMU (accelerometer / gyroscope) interface.

use crate::error::HalError;
use crate::types::Vec3;

/// Accelerometer / gyroscope / temperature (QMI8658 on the reference board).
pub trait ImuBackend: Send + Sync {
    /// Acceleration in g (including gravity).
    fn read_accel(&self) -> Result<Vec3, HalError>;
    /// Angular velocity in degrees per second.
    fn read_gyro(&self) -> Result<Vec3, HalError>;
    /// Die temperature in degrees Celsius.
    fn temperature_c(&self) -> Result<f32, HalError>;
}
