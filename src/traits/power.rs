//! Power / battery interface.

use crate::error::HalError;

/// Battery and power-supply management (AXP2101 PMIC on the reference board).
pub trait PowerBackend: Send + Sync {
    /// Initialize the PMIC / ADC subsystem.
    fn init(&mut self) -> Result<(), HalError>;
    /// Battery charge percentage in `0..=100`.
    fn battery_percent(&self) -> Result<u8, HalError>;
    /// Whether the battery is currently charging.
    fn is_charging(&self) -> Result<bool, HalError>;
    /// Battery voltage in millivolts.
    fn battery_voltage_mv(&self) -> Result<u32, HalError>;
}
