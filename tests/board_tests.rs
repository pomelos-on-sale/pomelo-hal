//! Integration tests for the HAL facade and the desktop simulator backends.
//!
//! These run on the host, where the board under test is [`Board::simulated`].

use std::sync::Arc;
use pomelo_hal::{Board, HalError, HttpBackend, ScanState, StorageBackend, WifiState};

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
    assert!(board.time().now_unix() > 0);
    assert!(!board.time().is_synced());
    assert!(matches!(
        board.storage().sd_status(),
        pomelo_hal::SdCardStatus::NotInserted | pomelo_hal::SdCardStatus::Mounted
    ));
    assert_eq!(board.http().state(), pomelo_hal::HttpState::Idle);
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

/// The credentials a test remembers.
fn credentials(ssid: &str, autoconnect: bool) -> pomelo_hal::wifi_credentials::WifiCredentials {
    pomelo_hal::wifi_credentials::WifiCredentials {
        enabled: true,
        ssid: ssid.to_string(),
        password: "hunter2".to_string(),
        autoconnect,
    }
}

/// What the file says is what the board does when it comes up.
#[test]
fn sim_wifi_boot_connects_to_the_remembered_network() {
    let board = Board::simulated();

    board
        .wifi()
        .remember(&credentials("Pomelo-OS", true))
        .unwrap();
    board.wifi().autoconnect().unwrap();

    let status = board.wifi().status();
    assert_eq!(status.state, WifiState::Connected);
    assert_eq!(status.ssid, "Pomelo-OS");
}

/// `autoconnect = false` is a network to keep, not one to join unasked.
#[test]
fn sim_wifi_boot_leaves_a_network_alone_when_the_file_says_to() {
    let board = Board::simulated();

    board
        .wifi()
        .remember(&credentials("Pomelo-OS", false))
        .unwrap();
    board.wifi().autoconnect().unwrap();

    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
    assert_eq!(
        board.wifi().saved().map(|saved| saved.ssid),
        Some("Pomelo-OS".to_string()),
        "still remembered, and still not joined"
    );
}

/// A board that has never been on a network comes up on nothing at all.
#[test]
fn sim_wifi_boot_without_a_file_joins_nothing() {
    let board = Board::simulated();

    board.wifi().autoconnect().unwrap();

    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
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

#[test]
fn board_event_listener_and_tick_change_detection() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let board = Board::simulated();
    let event_count = Arc::new(AtomicUsize::new(0));

    let count_clone = Arc::clone(&event_count);
    board.on_event(move |_event| {
        count_clone.fetch_add(1, Ordering::SeqCst);
    });

    // First tick detects initial state and emits events (Power & Wifi)
    board.tick();
    assert!(
        event_count.load(Ordering::SeqCst) >= 2,
        "initial tick should emit power and wifi status events"
    );

    // Explicit emit
    board.emit_event(pomelo_hal::SystemEvent::InputAction(pomelo_hal::InputAction::Back));
    assert!(event_count.load(Ordering::SeqCst) >= 3);

    // Test remove_listener
    let id = board.on_event(|_| {});
    assert!(board.remove_listener(id));
    assert!(!board.remove_listener(id));
}

#[test]
fn display_power_and_wake_swallowing() {
    let board = Board::simulated();
    assert!(board.is_display_on(), "display defaults to ON");

    // Drain initial battery & wifi events from first tick
    board.tick();
    while board.poll_event().is_some() {}

    // Turn off display (sleep)
    board.set_display_power(false).unwrap();
    assert!(!board.is_display_on(), "display should be OFF");

    // Simulate pressing a button while sleeping
    board.input().push_action(pomelo_hal::InputAction::Exit);

    // Tick: should wake up display, but swallow the Exit action!
    board.tick();
    assert!(board.is_display_on(), "display should have woken up");
    assert_eq!(board.poll_event(), None, "wake-up action must be swallowed, not queued");

    // Next action while awake should be delivered normally
    board.input().push_action(pomelo_hal::InputAction::Home);
    board.tick();
    assert_eq!(
        board.poll_event(),
        Some(pomelo_hal::SystemEvent::InputAction(pomelo_hal::InputAction::Home)),
        "subsequent Home action after wake must be dispatched normally"
    );

    board.input().push_action(pomelo_hal::InputAction::Back);
    board.tick();
    assert_eq!(
        board.poll_event(),
        Some(pomelo_hal::SystemEvent::InputAction(pomelo_hal::InputAction::Back)),
        "subsequent Back action after wake must be dispatched normally"
    );
}

#[test]
fn sim_storage_lifecycle_and_stats() {
    let board = Board::simulated();
    board.storage().mount_sd().unwrap();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::Mounted);
    let stats = board.storage().sd_stats().unwrap();
    assert!(stats.total_bytes > 0);
    assert!(stats.free_bytes <= stats.total_bytes);
    assert!(stats.used_bytes() <= stats.total_bytes);
    assert!(stats.used_percent() >= 0.0 && stats.used_percent() <= 100.0);

    board.storage().unmount_sd().unwrap();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::NotInserted);
    assert!(board.storage().sd_stats().is_err());
}

#[test]
fn sim_storage_auto_detect_hotplug() {
    let temp_dir = std::env::temp_dir().join(format!("pomelo_sim_sd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    use pomelo_hal::sim::SimStorage;
    let storage = SimStorage::with_path(&temp_dir);
    assert_eq!(storage.sd_status(), pomelo_hal::SdCardStatus::NotInserted);

    let board = Board::from_backends(
        Box::new(pomelo_hal::sim::SimPower::new()),
        Box::new(pomelo_hal::sim::SimWifi::new()),
        Box::new(pomelo_hal::sim::SimAudio::new()),
        Box::new(pomelo_hal::sim::SimMic::new()),
        Box::new(pomelo_hal::sim::SimImu::new()),
        Box::new(pomelo_hal::sim::SimInput::new()),
        Box::new(pomelo_hal::sim::SimTime::new()),
        Box::new(storage),
        Box::new(pomelo_hal::sim::SimHttp::new()),
    );

    board.tick();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::NotInserted);

    // Hot-plug simulation: directory appears
    std::fs::create_dir_all(&temp_dir).unwrap();
    board.tick();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::Mounted);

    // Pull-out simulation: directory is removed
    std::fs::remove_dir_all(&temp_dir).unwrap();
    board.tick();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::NotInserted);
}

#[test]
fn sim_storage_unformatted_and_format_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!("pomelo_sim_sd_unfmt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(temp_dir.join(".unformatted"), b"dummy").unwrap();

    use pomelo_hal::sim::SimStorage;
    let storage = SimStorage::with_path(&temp_dir);
    assert_eq!(storage.sd_status(), pomelo_hal::SdCardStatus::Unformatted);

    let board = Board::from_backends(
        Box::new(pomelo_hal::sim::SimPower::new()),
        Box::new(pomelo_hal::sim::SimWifi::new()),
        Box::new(pomelo_hal::sim::SimAudio::new()),
        Box::new(pomelo_hal::sim::SimMic::new()),
        Box::new(pomelo_hal::sim::SimImu::new()),
        Box::new(pomelo_hal::sim::SimInput::new()),
        Box::new(pomelo_hal::sim::SimTime::new()),
        Box::new(storage),
        Box::new(pomelo_hal::sim::SimHttp::new()),
    );

    let info = board.storage().sd_card_info().expect("info should be available for unformatted card");
    assert!(!info.is_formatted);
    assert!(info.total_bytes > 0);
    assert!(!info.name.is_empty());

    // Formatting on device
    board.storage().format_sd().unwrap();
    assert_eq!(board.storage().sd_status(), pomelo_hal::SdCardStatus::Mounted);

    let info2 = board.storage().sd_card_info().expect("info should be available after format");
    assert!(info2.is_formatted);
    assert_eq!(info2.total_bytes, info.total_bytes);

    let stats = board.storage().sd_stats().unwrap();
    assert!(stats.total_bytes > 0);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn board_http_lifecycle_and_tick() {
    let board = Board::simulated();
    assert_eq!(board.http().state(), pomelo_hal::HttpState::Idle);

    // Desktop system time is valid (>= 2026), so is_valid() is true and HTTPS works directly
    assert!(board.time().is_valid());
    assert!(board.http().start("https://pomelo.os/api/version").is_ok());
    assert!(matches!(board.http().state(), pomelo_hal::HttpState::Running { .. }));
    board.http().cancel().unwrap();

    // A cold-boot simulator with clock at epoch 0 and unsynced rejects HTTPS
    {
        use pomelo_hal::sim::{SimHttp, SimTime};
        let cold_time = Arc::new(std::sync::Mutex::new(SimTime::with_mock_time(0)));
        let mut cold_http = SimHttp::new();
        cold_http.set_time_source(cold_time as Arc<std::sync::Mutex<dyn pomelo_hal::TimeBackend>>);

        assert_eq!(
            cold_http.start("https://pomelo.os/api/version"),
            Err(HalError::ClockNotSynced)
        );
    }

    // HTTP works
    assert!(board.http().start("http://pomelo.os/api/version").is_ok());
    assert!(matches!(board.http().state(), pomelo_hal::HttpState::Running { .. }));

    // Tick advances state
    board.tick();
    board.tick();
    assert_eq!(board.http().state(), pomelo_hal::HttpState::Done);
    assert_eq!(board.http().status_code(), Some(200));

    let data = board.http().take_result().unwrap();
    assert!(!data.is_empty());
    assert_eq!(board.http().state(), pomelo_hal::HttpState::Idle);
}

#[test]
fn backup_and_restore_app_data_roundtrip() {
    use pomelo_hal::backup::{
        backup_app_data_to_file, has_backup_at_file, restore_app_data_from_file,
    };
    use pomelo_hal::storage::{kv_get_raw, kv_set_raw, kv_clear_all};

    let test_dir = std::env::temp_dir().join(format!("pomelo_backup_test_{}", std::process::id()));
    let backup_file = test_dir.join("sdcard/Backup/app_data.json");

    let _ = std::fs::remove_dir_all(&test_dir);
    std::env::set_var("POMELO_APP_DATA", &test_dir);

    // 1. Write KV configurations
    kv_set_raw("WIFI", "credentials", b"{\"ssid\":\"HomeNet\",\"enabled\":true}").unwrap();
    kv_set_raw("SETTINGS", "preferences", b"{\"dark\":true}").unwrap();

    assert!(!has_backup_at_file(&backup_file));

    // 2. Perform backup
    let stats = backup_app_data_to_file(&backup_file).expect("backup should succeed");
    assert!(stats.files_count >= 2);
    assert!(stats.bytes_count > 0);
    assert!(has_backup_at_file(&backup_file));

    // 3. Clear KV
    kv_clear_all().unwrap();
    assert_eq!(kv_get_raw("WIFI", "credentials").unwrap(), None);

    // 4. Perform restore
    let restore_stats = restore_app_data_from_file(&backup_file).expect("restore should succeed");
    assert!(restore_stats.files_count >= 2);

    // 5. Verify restored state
    let wifi_bytes = kv_get_raw("WIFI", "credentials").unwrap().expect("wifi credentials must exist");
    let wifi_val: serde_json::Value = serde_json::from_slice(&wifi_bytes).unwrap();
    assert_eq!(wifi_val["ssid"], "HomeNet");
    assert_eq!(wifi_val["enabled"], true);

    let settings_bytes = kv_get_raw("SETTINGS", "preferences").unwrap().expect("settings preferences must exist");
    let settings_val: serde_json::Value = serde_json::from_slice(&settings_bytes).unwrap();
    assert_eq!(settings_val["dark"], true);

    std::env::remove_var("POMELO_APP_DATA");
    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn backup_error_handling_for_nonexistent_dirs() {
    use pomelo_hal::backup::restore_app_data_from_file;

    let test_dir = std::env::temp_dir().join(format!("pomelo_backup_err_{}", std::process::id()));
    let non_existent = test_dir.join("ghost_file.json");

    assert!(restore_app_data_from_file(&non_existent).is_err());

    let _ = std::fs::remove_dir_all(&test_dir);
}


