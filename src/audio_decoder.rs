//! Unified audio decoding and metadata extraction powered by Symphonia.
//!
//! Supports streaming decode of both `.mp3` and `.wav` formats into raw
//! 16-bit interleaved PCM audio suitable for DACs / I2S audio sinks.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::HalError;
use crate::types::AudioMeta;

/// A streaming audio decoder that decodes compressed audio (e.g. MP3, WAV)
/// into a raw 16-bit interleaved little-endian PCM byte stream via [`std::io::Read`].
pub struct AudioDecoder {
    format: Box<dyn FormatReader + Send>,
    decoder: Box<dyn Decoder + Send>,
    track_id: u32,
    meta: AudioMeta,
    sample_buf: Option<SampleBuffer<i16>>,
    sample_offset: usize,
}

impl AudioDecoder {
    /// Opens an audio file by path, probes its format, and initializes the decoder.
    pub fn open(path: &str) -> Result<Self, HalError> {
        let file = File::open(path)
            .map_err(|e| HalError::Io(format!("failed to open '{path}': {e}")))?;
        Self::from_file(file, Some(path))
    }

    /// Creates a decoder from a [`File`] handle, optionally passing the original path for hints.
    pub fn from_file(file: File, path_hint: Option<&str>) -> Result<Self, HalError> {
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(p) = path_hint {
            if let Some(ext) = Path::new(p).extension().and_then(|e| e.to_str()) {
                hint.with_extension(ext);
            }
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| HalError::Io(format!("audio format probe failed: {e}")))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| HalError::Io("no supported audio track found".to_string()))?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map_or(2, |c| c.count() as u8);
        let bits_per_sample = 16u8;

        // Determine or estimate duration
        let duration_secs = if let (Some(n_frames), Some(tb)) =
            (track.codec_params.n_frames, track.codec_params.time_base)
        {
            let time = tb.calc_time(n_frames);
            time.seconds as f32 + time.frac as f32
        } else if file_len > 0 {
            // Fallback estimation for typical MP3s (128 kbps)
            (file_len as f32 * 8.0) / 128_000.0
        } else {
            0.0
        };

        let meta = AudioMeta {
            sample_rate,
            channels,
            bits_per_sample,
            duration_secs,
        };

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| HalError::Io(format!("failed to instantiate decoder: {e}")))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            meta,
            sample_buf: None,
            sample_offset: 0,
        })
    }

    /// Returns the probed metadata of the audio stream.
    pub fn meta(&self) -> &AudioMeta {
        &self.meta
    }
}

impl Read for AudioDecoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut bytes_written = 0;

        loop {
            // 1. Drain any already-decoded PCM samples in sample_buf
            if let Some(ref sbuf) = self.sample_buf {
                let samples = sbuf.samples();
                while self.sample_offset < samples.len() && bytes_written + 2 <= buf.len() {
                    let sample = samples[self.sample_offset];
                    let le_bytes = sample.to_le_bytes();
                    buf[bytes_written] = le_bytes[0];
                    buf[bytes_written + 1] = le_bytes[1];
                    bytes_written += 2;
                    self.sample_offset += 1;
                }

                if self.sample_offset >= samples.len() {
                    self.sample_buf = None;
                    self.sample_offset = 0;
                }

                // If we filled the destination buffer, return bytes read
                if bytes_written > 0 {
                    return Ok(bytes_written);
                }
            }

            // 2. Fetch the next packet from the container demuxer
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != self.track_id {
                        continue;
                    }

                    match self.decoder.decode(&packet) {
                        Ok(decoded) => {
                            let spec = *decoded.spec();
                            let duration = decoded.capacity() as u64;
                            let mut sbuf = SampleBuffer::<i16>::new(duration, spec);
                            sbuf.copy_interleaved_ref(decoded);
                            self.sample_buf = Some(sbuf);
                            self.sample_offset = 0;
                        }
                        Err(SymphoniaError::DecodeError(e)) => {
                            // Non-fatal decode error in stream, skip packet
                            eprintln!("[AudioDecoder] decode warning: {e}");
                            continue;
                        }
                        Err(e) => {
                            return Err(io::Error::new(io::ErrorKind::Other, e.to_string()));
                        }
                    }
                }
                Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    // Normal EOF
                    return Ok(bytes_written);
                }
                Err(SymphoniaError::IoError(e)) => {
                    return Err(e);
                }
                Err(_) => {
                    // End of stream
                    return Ok(bytes_written);
                }
            }
        }
    }
}

/// Probes an audio file (WAV or MP3) and returns its [`AudioMeta`].
pub fn probe_audio_meta(path: &str) -> Result<AudioMeta, HalError> {
    AudioDecoder::open(path).map(|d| *d.meta())
}

/// Format a duration in seconds as `MM:SS`.
pub fn format_time(seconds: f32) -> String {
    let total_secs = seconds.max(0.0) as u32;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_and_probe_mp3_and_wav() {
        let candidates = [
            "../assets/internal/sounds/creatorassets.com/sound-1.mp3",
            "../../assets/internal/sounds/creatorassets.com/sound-1.mp3",
            "assets/internal/sounds/creatorassets.com/sound-1.mp3",
        ];
        let mp3_path = candidates.iter().find(|p| Path::new(p).exists());
        if let Some(path) = mp3_path {
            let meta = probe_audio_meta(path).expect("probe mp3 should succeed");
            assert!(meta.sample_rate > 0, "sample rate must be positive");
            assert!(meta.channels > 0, "channels must be >= 1");
            assert_eq!(meta.bits_per_sample, 16);
            assert!(meta.duration_secs > 0.0, "duration must be positive");

            let mut decoder = AudioDecoder::open(path).expect("open mp3 decoder");
            let mut buf = [0u8; 4096];
            let mut total_bytes = 0;
            while let Ok(n) = decoder.read(&mut buf) {
                if n == 0 {
                    break;
                }
                total_bytes += n;
            }
            assert!(total_bytes > 0, "must decode pcm bytes from mp3");
            assert_eq!(total_bytes % 2, 0, "16-bit PCM bytes must be even");
        }

        let wav_candidates = [
            "../assets/music/Carefree - Kevin MacLeod.wav",
            "../../assets/music/Carefree - Kevin MacLeod.wav",
            "assets/music/Carefree - Kevin MacLeod.wav",
        ];
        let wav_path = wav_candidates.iter().find(|p| Path::new(p).exists());
        if let Some(path) = wav_path {
            let meta = probe_audio_meta(path).expect("probe wav should succeed");
            assert_eq!(meta.sample_rate, 22050);
            assert!(meta.channels >= 1);
            assert_eq!(meta.bits_per_sample, 16);
            assert!(meta.duration_secs > 0.0);

            let mut decoder = AudioDecoder::open(path).expect("open wav decoder");
            let mut buf = [0u8; 4096];
            let mut total_bytes = 0;
            while let Ok(n) = decoder.read(&mut buf) {
                if n == 0 {
                    break;
                }
                total_bytes += n;
            }
            assert!(total_bytes > 0, "must decode pcm bytes from wav");
        }
    }
}

