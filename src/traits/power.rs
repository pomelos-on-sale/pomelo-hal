//! Power / battery interface.

use std::time::Duration;

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
    /// Turns display panel power on or off. Default is a no-op returning `Ok(())`.
    fn set_display_power(&mut self, _on: bool) -> Result<(), HalError> {
        Ok(())
    }
    /// Whether the display panel is currently powered on.
    fn is_display_on(&self) -> Result<bool, HalError> {
        Ok(true)
    }
    /// Sets display auto-sleep inactivity timeout. `None` means never sleep.
    fn set_sleep_timeout(&mut self, _timeout: Option<Duration>) -> Result<(), HalError> {
        Ok(())
    }
    /// Inactivity timeout before display automatically sleeps. `None` means never sleep.
    fn sleep_timeout(&self) -> Result<Option<Duration>, HalError> {
        Ok(None)
    }
}
