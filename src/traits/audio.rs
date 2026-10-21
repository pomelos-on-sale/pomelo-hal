//! Audio playback interface.

use crate::error::HalError;
use crate::types::AudioMeta;

/// PCM audio playback.
pub trait AudioBackend: Send + Sync {
    /// Start playback of the file at `path`.
    fn play(&mut self, path: &str) -> Result<AudioMeta, HalError>;

    /// Start playback of the file at `path`, looping continuously until explicitly stopped.
    fn play_loop(&mut self, path: &str) -> Result<AudioMeta, HalError> {
        self.play(path)
    }

    /// Play a pre-decoded raw 16-bit PCM buffer from memory.
    fn play_pcm(
        &mut self,
        _pcm: std::sync::Arc<Vec<u8>>,
        _sample_rate: u32,
        _channels: u8,
        _bits_per_sample: u8,
    ) -> Result<AudioMeta, HalError> {
        Err(HalError::NotSupported)
    }

    /// Play compressed audio bytes (e.g. embedded static MP3/WAV asset) from memory.
    fn play_bytes(
        &mut self,
        _bytes: &'static [u8],
        _format_hint: Option<&str>,
    ) -> Result<AudioMeta, HalError> {
        Err(HalError::NotSupported)
    }

    /// Play compressed audio bytes from memory in a continuous loop.
    fn play_bytes_loop(
        &mut self,
        _bytes: &'static [u8],
        _format_hint: Option<&str>,
    ) -> Result<AudioMeta, HalError> {
        Err(HalError::NotSupported)
    }

    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    fn set_volume(&mut self, volume: u8);
    /// Current volume level (0..=100). Defaults to 75.
    fn volume(&self) -> u8 {
        75
    }
    fn is_playing(&self) -> bool;
    /// Playback position in seconds.
    fn position_secs(&self) -> f32;
    /// Poll for end-of-stream and internal state transitions.
    fn tick(&mut self);
}
