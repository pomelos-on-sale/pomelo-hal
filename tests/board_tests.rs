//! Integration tests for the HAL facade and the desktop simulator backends.
//!
//! These run on the host, where the board under test is [`Board::simulated`].

use pomelo_hal::{Board, HalError, ScanState, WifiState};

/// Write a minimal 16-bit mono PCM WAV (0.1 s @ 8 kHz) to a temp file and
/// return its path, so the audio backend has a real file to parse.
fn write_temp_wav() -> String {
    let data_len: u32 = 1600; // 8000 Hz * 2 bytes * 0.1 s
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // channels
    b.extend_from_slice(&8000u32.to_le_bytes()); // sample rate
    b.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 0);

    let path = std::env::temp_dir().join("pomelo_hal_sim_audio.wav");
    std::fs::write(&path, &b).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn board_constructs_and_exposes_every_subsystem() {
    let board = Board::simulated();
    // Every accessor must be reachable without panicking or deadlocking.
    assert!(board.power().battery_percent().is_ok());
    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
    assert!(!board.audio().is_playing());
    assert!(!board.mic().is_recording());
    assert!(board.imu().read_accel().is_ok());
    assert_eq!(board.input().poll_action(), None);
    board.input().push_action(pomelo_hal::InputAction::Back);
    assert_eq!(board.input().poll_action(), Some(pomelo_hal::InputAction::Back));
    board.tick();
}

#[test]
fn sim_power_reports_valid_ranges() {
    let board = Board::simulated();
    board.power().init().unwrap();

    let pct = board.power().battery_percent().unwrap();
    assert!(pct <= 100, "battery percent must be <= 100, got {pct}");

    let voltage = board.power().battery_voltage_mv().unwrap();
    assert!(
        (3300..=4200).contains(&voltage),
        "voltage must map into 3300..=4200 mV, got {voltage}"
    );

    let _charging = board.power().is_charging().unwrap();
}

#[test]
fn sim_wifi_scan_lifecycle() {
    let board = Board::simulated();
    board.wifi().init().unwrap();
    assert!(board.wifi().is_enabled());
    assert_eq!(board.wifi().scan_state(), ScanState::Idle);

    board.wifi().scan_start().unwrap();
    assert_eq!(board.wifi().scan_state(), ScanState::Scanning);

    // Results are not ready mid-scan.
    assert!(matches!(board.wifi().scan_results(), Err(HalError::Busy)));

    // Cooperative ticks drive the simulated scan to completion.
    for _ in 0..3 {
        board.wifi().tick();
    }
    assert_eq!(board.wifi().scan_state(), ScanState::Done);

    let aps = board.wifi().scan_results().unwrap();
    assert!(aps.len() >= 3, "expected several simulated APs");
    assert!(
        aps.iter().any(|ap| ap.ssid == "Pomelo-OS"),
        "expected the Pomelo-OS AP in scan results"
    );
    // Bars are derived from RSSI and must stay within the icon range.
    for ap in &aps {
        assert!(ap.signal_bars() <= 4);
    }
}

#[test]
fn sim_wifi_connect_requires_valid_credentials() {
    let board = Board::simulated();

    // Unknown SSID is rejected.
    assert!(matches!(
        board.wifi().connect("Nope", "secret"),
        Err(HalError::InvalidArg)
    ));

    // A secured AP requires a password.
    assert!(matches!(
        board.wifi().connect("Pomelo-OS", ""),
        Err(HalError::InvalidArg)
    ));

    // A valid connection flips the status to Connected.
    board.wifi().connect("Pomelo-OS", "hunter2").unwrap();
    let status = board.wifi().status();
    assert_eq!(status.state, WifiState::Connected);
    assert_eq!(status.ssid, "Pomelo-OS");
    assert_eq!(status.ip, "192.168.1.108");

    board.wifi().disconnect().unwrap();
    assert_eq!(board.wifi().status().state, WifiState::Disconnected);

    // Disabling the radio rejects scanning.
    board.wifi().set_enabled(false).unwrap();
    assert!(matches!(
        board.wifi().scan_start(),
        Err(HalError::NotInitialized)
    ));
}

#[test]
fn sim_audio_playback_lifecycle() {
    let board = Board::simulated();
    let path = write_temp_wav();

    let meta = board.audio().play(&path).unwrap();
    assert_eq!(meta.sample_rate, 8000);
    assert_eq!(meta.channels, 1);
    assert_eq!(meta.bits_per_sample, 16);
    assert!(board.audio().is_playing());

    board.audio().pause();
    assert!(!board.audio().is_playing());

    board.audio().resume();
    assert!(board.audio().is_playing());

    board.audio().stop();
    assert!(!board.audio().is_playing());
    assert_eq!(board.audio().position_secs(), 0.0);

    // A missing file must surface as an error rather than a panic.
    assert!(board.audio().play("/no/such/file.wav").is_err());
}

#[test]
fn sim_mic_capture() {
    let board = Board::simulated();
    assert!(!board.mic().is_recording());

    // Reading before starting must fail.
    let mut buf = [0i16; 64];
    assert!(board.mic().read(&mut buf).is_err());

    board.mic().record_start().unwrap();
    assert!(board.mic().is_recording());

    let n = board.mic().read(&mut buf).unwrap();
    assert_eq!(n, buf.len());
    assert!(
        buf.iter().any(|&s| s != 0),
        "simulated microphone should produce non-silent samples"
    );

    board.mic().record_stop().unwrap();
    assert!(!board.mic().is_recording());
}

#[test]
fn sim_imu_reads_finite_values() {
    let board = Board::simulated();

    let accel = board.imu().read_accel().unwrap();
    let magnitude = accel.magnitude();
    assert!(
        (9.0..=11.0).contains(&magnitude),
        "gravity magnitude should be near 9.81, got {magnitude}"
    );

    let gyro = board.imu().read_gyro().unwrap();
    assert!(gyro.magnitude() < 1.0);

    let temp = board.imu().temperature_c().unwrap();
    assert!((20.0..=45.0).contains(&temp), "unexpected temp {temp}");
}
