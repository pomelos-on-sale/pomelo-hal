//! Simulated battery (AXP2101 PMIC).

use std::time::{Duration, Instant};

use crate::error::HalError;
use crate::traits::PowerBackend;

/// Simulated battery that slowly drains while the app is running.
pub struct SimPower {
    start: Instant,
    initial_percent: u8,
    secs_per_percent: f32,
    charging: bool,
    display_on: bool,
    sleep_timeout: Option<Duration>,
}

impl SimPower {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            initial_percent: 88,
            secs_per_percent: 45.0,
            charging: true,
            display_on: true,
            sleep_timeout: None,
        }
    }

    fn percent(&self) -> u8 {
        let drained = (self.start.elapsed().as_secs_f32() / self.secs_per_percent) as u8;
        self.initial_percent.saturating_sub(drained).max(1)
    }
}

impl Default for SimPower {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerBackend for SimPower {
    fn init(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    fn battery_percent(&self) -> Result<u8, HalError> {
        Ok(self.percent())
    }

    fn is_charging(&self) -> Result<bool, HalError> {
        Ok(self.charging)
    }

    fn battery_voltage_mv(&self) -> Result<u32, HalError> {
        // Linear map 0%..100% -> 3300..4200 mV.
        Ok(3300 + self.percent() as u32 * 9)
    }

    fn set_display_power(&mut self, on: bool) -> Result<(), HalError> {
        self.display_on = on;
        Ok(())
    }

    fn is_display_on(&self) -> Result<bool, HalError> {
        Ok(self.display_on)
    }

    fn set_sleep_timeout(&mut self, timeout: Option<Duration>) -> Result<(), HalError> {
        self.sleep_timeout = timeout;
        Ok(())
    }

    fn sleep_timeout(&self) -> Result<Option<Duration>, HalError> {
        Ok(self.sleep_timeout)
    }

    fn reboot(&mut self) -> Result<(), HalError> {
        eprintln!("[sim_power] system reboot requested");
        Ok(())
    }
}
