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
    /// Sets display auto-screen-off inactivity timeout. `None` means never turn off automatically.
    fn set_screen_off_timeout(&mut self, _timeout: Option<Duration>) -> Result<(), HalError> {
        Ok(())
    }
    /// Inactivity timeout before display automatically turns off. `None` means never turn off.
    fn screen_off_timeout(&self) -> Result<Option<Duration>, HalError> {
        Ok(None)
    }
    /// Sets display brightness level in percentage (10..=100%). Default is a no-op returning `Ok(())`.
    fn set_display_brightness(&mut self, _percent: u8) -> Result<(), HalError> {
        Ok(())
    }
    /// Current display brightness level in percentage (10..=100%). Default returns `Ok(100)`.
    fn display_brightness(&self) -> Result<u8, HalError> {
        Ok(100)
    }
    /// Reboots the system / device. Default implementation is a no-op returning `Ok(())`.
    fn reboot(&mut self) -> Result<(), HalError> {
        Ok(())
    }
    /// Whether USB (VBUS) power is currently connected. Default returns `Ok(false)`.
    fn is_usb_connected(&self) -> Result<bool, HalError> {
        Ok(false)
    }
    /// Whether stay-awake on USB is enabled. Default returns `Ok(true)`.
    fn stay_awake_on_usb(&self) -> Result<bool, HalError> {
        Ok(true)
    }
    /// Configures whether to stay awake when USB is connected. Default returns `Ok(())`.
    fn set_stay_awake_on_usb(&mut self, _enable: bool) -> Result<(), HalError> {
        Ok(())
    }
    /// Whether keeping screen on (wake lock) is requested. Default returns `Ok(false)`.
    fn is_keep_screen_on(&self) -> Result<bool, HalError> {
        Ok(false)
    }
    /// Requests or releases keeping the screen on. Default returns `Ok(())`.
    fn set_keep_screen_on(&mut self, _enable: bool) -> Result<(), HalError> {
        Ok(())
    }
}
