//! Simulated audio backend.
//!
//! With the `simulator` feature enabled it plays real files through `rodio` on
//! a dedicated worker thread (the host audio stream is not `Send`); without the
//! feature it tracks playback position only and stays silent. Either way it
//! parses the WAV header so the returned metadata (and hence the UI progress
//! bar) matches the real file.

use std::fs::File;
use std::io::Read;
use std::time::Instant;

use crate::error::HalError;
use crate::traits::AudioBackend;
use crate::types::AudioMeta;
use crate::wav::parse_wav_header;

#[cfg(feature = "simulator")]
use std::sync::mpsc::{channel, Sender};

/// Commands sent to the rodio worker thread. Defined unconditionally so the
/// `send` call sites stay identical whether or not the `simulator` feature is on.
#[allow(dead_code)]
enum SimAudioCommand {
    Play { path: String, volume: f32 },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
}

pub struct SimAudio {
    #[cfg(feature = "simulator")]
    tx: Option<Sender<SimAudioCommand>>,
    is_playing: bool,
    paused: bool,
    volume: u8,
    elapsed_base: f32,
    started_at: Option<Instant>,
    duration: f32,
}

impl SimAudio {
    pub fn new() -> Self {
        #[cfg(feature = "simulator")]
        let tx = {
            let (sender, receiver) = channel::<SimAudioCommand>();
            std::thread::Builder::new()
                .name("sim_audio_thread".into())
                .spawn(move || {
                    let (_stream, sink) = match rodio::OutputStream::try_default() {
                        Ok((stream, handle)) => (Some(stream), rodio::Sink::try_new(&handle).ok()),
                        Err(e) => {
                            eprintln!("[SimAudio] no host audio device: {e}");
                            (None, None)
                        }
                    };

                    while let Ok(cmd) = receiver.recv() {
                        match cmd {
                            SimAudioCommand::Play { path, volume } => {
                                if let Some(sink) = &sink {
                                    sink.stop();
                                    if let Ok(file) = File::open(&path) {
                                        if let Ok(source) =
                                            rodio::Decoder::new(std::io::BufReader::new(file))
                                        {
                                            sink.set_volume(volume);
                                            sink.append(source);
                                            sink.play();
                                        }
                                    }
                                }
                            }
                            SimAudioCommand::Pause => {
                                if let Some(sink) = &sink {
                                    sink.pause();
                                }
                            }
                            SimAudioCommand::Resume => {
                                if let Some(sink) = &sink {
                                    sink.play();
                                }
                            }
                            SimAudioCommand::Stop => {
                                if let Some(sink) = &sink {
                                    sink.stop();
                                }
                            }
                            SimAudioCommand::SetVolume(v) => {
                                if let Some(sink) = &sink {
                                    sink.set_volume(v);
                                }
                            }
                        }
                    }
                })
                .ok();
            Some(sender)
        };

        Self {
            #[cfg(feature = "simulator")]
            tx,
            is_playing: false,
            paused: false,
            volume: 75,
            elapsed_base: 0.0,
            started_at: None,
            duration: 0.0,
        }
    }

    #[cfg(feature = "simulator")]
    fn send(&self, cmd: SimAudioCommand) {
        if let Some(ref tx) = self.tx {
            let _ = tx.send(cmd);
        }
    }

    #[cfg(not(feature = "simulator"))]
    fn send(&self, _cmd: SimAudioCommand) {}

    fn elapsed(&self) -> f32 {
        let mut e = self.elapsed_base;
        if let Some(t) = self.started_at {
            e += t.elapsed().as_secs_f32();
        }
        e
    }
}

impl Default for SimAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for SimAudio {
    fn play(&mut self, path: &str) -> Result<AudioMeta, HalError> {
        let mut file =
            File::open(path).map_err(|e| HalError::Io(format!("failed to open '{path}': {e}")))?;
        let mut header_buf = [0u8; 512];
        let bytes_read = file
            .read(&mut header_buf)
            .map_err(|e| HalError::Io(e.to_string()))?;
        let meta = parse_wav_header(&header_buf[..bytes_read]).map_err(HalError::Io)?;

        self.duration = meta.duration_secs;
        self.elapsed_base = 0.0;
        self.started_at = Some(Instant::now());
        self.paused = false;
        self.is_playing = true;

        self.send(SimAudioCommand::Play {
            path: path.to_string(),
            volume: self.volume as f32 / 100.0,
        });

        Ok(AudioMeta {
            sample_rate: meta.sample_rate,
            channels: meta.channels as u8,
            bits_per_sample: meta.bits_per_sample as u8,
            duration_secs: meta.duration_secs,
        })
    }

    fn pause(&mut self) {
        if self.is_playing && !self.paused {
            self.elapsed_base = self.elapsed();
            self.started_at = None;
            self.paused = true;
            self.send(SimAudioCommand::Pause);
        }
    }

    fn resume(&mut self) {
        if self.paused {
            self.started_at = Some(Instant::now());
            self.paused = false;
            self.send(SimAudioCommand::Resume);
        }
    }

    fn stop(&mut self) {
        self.is_playing = false;
        self.paused = false;
        self.elapsed_base = 0.0;
        self.started_at = None;
        self.send(SimAudioCommand::Stop);
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.send(SimAudioCommand::SetVolume(self.volume as f32 / 100.0));
    }

    fn is_playing(&self) -> bool {
        self.is_playing && !self.paused
    }

    fn position_secs(&self) -> f32 {
        self.elapsed().min(self.duration)
    }

    fn tick(&mut self) {
        if self.is_playing() && self.duration > 0.0 && self.position_secs() >= self.duration {
            self.stop();
        }
    }
}
