//! Zero-dependency RIFF WAV parser, shared by the native and simulator audio
//! backends (and re-exported for applications that want to display metadata).

/// Parsed metadata for a RIFF/WAVE stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavMetadata {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub byte_rate: u32,
    pub data_offset: usize,
    pub data_len: usize,
    pub duration_secs: f32,
}

impl WavMetadata {
    pub fn format_duration(&self) -> String {
        format_time(self.duration_secs)
    }
}

/// Format a duration in seconds as `MM:SS`.
pub fn format_time(seconds: f32) -> String {
    let total_secs = seconds.max(0.0) as u32;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

/// Parse the header of a RIFF/WAVE buffer, returning its format metadata.
pub fn parse_wav_header(bytes: &[u8]) -> Result<WavMetadata, String> {
    if bytes.len() < 44 {
        return Err("File too small to be a valid WAV (< 44 bytes)".to_string());
    }

    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Invalid RIFF/WAVE header".to_string());
    }

    let mut cursor = 12;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut byte_rate = 0u32;
    let mut data_offset = 0usize;
    let mut data_len = 0usize;
    let mut found_fmt = false;

    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;

        if chunk_id == b"fmt " {
            if chunk_size < 14 || cursor + chunk_size > bytes.len() {
                return Err("Malformed fmt chunk in WAV".to_string());
            }
            let _audio_format = u16::from_le_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
            channels = u16::from_le_bytes(bytes[cursor + 2..cursor + 4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap());
            byte_rate = u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().unwrap());
            let _block_align =
                u16::from_le_bytes(bytes[cursor + 12..cursor + 14].try_into().unwrap());
            if chunk_size >= 16 {
                bits_per_sample =
                    u16::from_le_bytes(bytes[cursor + 14..cursor + 16].try_into().unwrap());
            } else {
                bits_per_sample = 16;
            }
            found_fmt = true;
            cursor += chunk_size;
        } else if chunk_id == b"data" {
            data_offset = cursor;
            data_len = chunk_size;
            break; // Typically data is the main payload
        } else {
            // Skip unknown chunk (e.g. LIST, JUNK, ID3)
            cursor += chunk_size;
        }
    }

    if !found_fmt {
        return Err("Missing 'fmt ' chunk in WAV file".to_string());
    }

    if byte_rate == 0 {
        byte_rate = sample_rate * (channels as u32) * ((bits_per_sample as u32) / 8);
    }

    let duration_secs = if byte_rate > 0 {
        data_len as f32 / byte_rate as f32
    } else {
        0.0
    };

    Ok(WavMetadata {
        channels,
        sample_rate,
        bits_per_sample,
        byte_rate,
        data_offset,
        data_len,
        duration_secs,
    })
}
