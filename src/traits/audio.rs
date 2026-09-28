//! Audio playback interface.

use crate::error::HalError;
use crate::types::AudioMeta;

/// PCM audio playback.
pub trait AudioBackend: Send + Sync {
    /// Start playback of the file at `path`.
    fn play(&mut self, path: &str) -> Result<AudioMeta, HalError>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    fn set_volume(&mut self, volume: u8);
    fn is_playing(&self) -> bool;
    /// Playback position in seconds.
    fn position_secs(&self) -> f32;
    /// Poll for end-of-stream and internal state transitions.
    fn tick(&mut self);
}
