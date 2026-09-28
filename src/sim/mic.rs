//! Simulated microphone that emits a quiet sine wave.

use std::f32::consts::PI;

use crate::error::HalError;
use crate::traits::MicBackend;

pub struct SimMic {
    recording: bool,
    phase: f32,
}

impl SimMic {
    pub fn new() -> Self {
        Self {
            recording: false,
            phase: 0.0,
        }
    }
}

impl Default for SimMic {
    fn default() -> Self {
        Self::new()
    }
}

impl MicBackend for SimMic {
    fn record_start(&mut self) -> Result<(), HalError> {
        self.recording = true;
        Ok(())
    }

    fn read(&mut self, buf: &mut [i16]) -> Result<usize, HalError> {
        if !self.recording {
            return Err(HalError::NotInitialized);
        }
        for sample in buf.iter_mut() {
            *sample = (f32::sin(self.phase) * 8000.0) as i16;
            self.phase += 0.05;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
        Ok(buf.len())
    }

    fn record_stop(&mut self) -> Result<(), HalError> {
        self.recording = false;
        Ok(())
    }

    fn is_recording(&self) -> bool {
        self.recording
    }
}
