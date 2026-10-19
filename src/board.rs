//! The [`Board`] facade aggregates every subsystem backend.
//!
//! It names no implementation. Which backends a board has is the *composition root's*
//! decision — `pomelo_hal_esp32::board()` builds this board's hardware on the device,
//! [`Board::simulated`] builds the desktop simulator, and a test may build its own — and
//! the resulting `Arc<Board>` is what application code receives. That is why this crate
//! carries no `extern "C"` and no `target_os` switch: the choice is an argument, not a
//! `#[cfg]`.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use crate::traits::{
    AudioBackend, HttpBackend, ImuBackend, InputBackend, MicBackend, PowerBackend, StorageBackend,
    TimeBackend, WifiBackend,
};
use crate::types::SystemEvent;
#[cfg(not(target_os = "espidf"))]
use crate::types::{SdCardStatus, WifiStatus};

pub type EventListener = Arc<dyn Fn(&SystemEvent) + Send + Sync>;

const MAX_EVENT_QUEUE_SIZE: usize = 32;

#[cfg(not(target_os = "espidf"))]
#[derive(Default)]
struct EventStateCache {
    last_battery: Option<u8>,
    last_charging: Option<bool>,
    last_wifi_status: Option<WifiStatus>,
    last_sd_status: Option<SdCardStatus>,
}

/// A shared handle to every hardware subsystem.
///
/// Assemble one with [`Board::from_backends`] (or [`Board::simulated`]) and pass the
/// resulting `Arc<Board>` down into application/UI code. Each backend is guarded by its own
/// mutex, so a lock on one subsystem never blocks another.
pub struct Board {
    power: Mutex<Box<dyn PowerBackend>>,
    wifi: Mutex<Box<dyn WifiBackend>>,
    audio: Mutex<Box<dyn AudioBackend>>,
    mic: Mutex<Box<dyn MicBackend>>,
    imu: Mutex<Box<dyn ImuBackend>>,
    input: Mutex<Box<dyn InputBackend>>,
    time: Mutex<Box<dyn TimeBackend>>,
    storage: Mutex<Box<dyn StorageBackend>>,
    http: Mutex<Box<dyn HttpBackend>>,
    listeners: Mutex<Vec<(u64, EventListener)>>,
    next_listener_id: std::sync::atomic::AtomicUsize,
    events: Mutex<VecDeque<SystemEvent>>,
    event_condvar: Condvar,
    #[cfg(not(target_os = "espidf"))]
    cache: Mutex<EventStateCache>,
    wakeup: Arc<crate::wakeup::WakeupScheduler>,
}

impl Board {
    /// Assemble a board from one backend per subsystem.
    ///
    /// The order is `power`, `wifi`, `audio`, `mic`, `imu`, `input`, `time`, `storage`, `http` — the same order the accessors
    /// appear in. It takes boxes rather than generics so the caller can mix concrete types
    /// and, in a test, its own fakes.
    #[allow(clippy::too_many_arguments)]
    pub fn from_backends(
        power: Box<dyn PowerBackend>,
        wifi: Box<dyn WifiBackend>,
        audio: Box<dyn AudioBackend>,
        mic: Box<dyn MicBackend>,
        imu: Box<dyn ImuBackend>,
        input: Box<dyn InputBackend>,
        time: Box<dyn TimeBackend>,
        storage: Box<dyn StorageBackend>,
        http: Box<dyn HttpBackend>,
    ) -> Self {
        Self {
            power: Mutex::new(power),
            wifi: Mutex::new(wifi),
            audio: Mutex::new(audio),
            mic: Mutex::new(mic),
            imu: Mutex::new(imu),
            input: Mutex::new(input),
            time: Mutex::new(time),
            storage: Mutex::new(storage),
            http: Mutex::new(http),
            listeners: Mutex::new(Vec::new()),
            next_listener_id: std::sync::atomic::AtomicUsize::new(1),
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENT_QUEUE_SIZE)),
            event_condvar: Condvar::new(),
            #[cfg(not(target_os = "espidf"))]
            cache: Mutex::new(EventStateCache::default()),
            wakeup: Arc::new(crate::wakeup::WakeupScheduler::new()),
        }
    }

    /// A board of desktop simulator backends.
    #[cfg(not(target_os = "espidf"))]
    pub fn simulated() -> Self {
        use crate::sim::{
            SimAudio, SimHttp, SimImu, SimInput, SimMic, SimPower, SimStorage, SimTime, SimWifi,
        };

        let time_shared: Arc<Mutex<dyn TimeBackend>> = Arc::new(Mutex::new(SimTime::new()));
        let mut http = SimHttp::new();
        http.set_time_source(Arc::clone(&time_shared));

        struct SharedTime(Arc<Mutex<dyn TimeBackend>>);
        impl TimeBackend for SharedTime {
            fn sync(&mut self, server: Option<&str>, timeout: Duration) -> Result<u64, crate::error::HalError> {
                lock(&self.0).sync(server, timeout)
            }
            fn is_synced(&self) -> bool {
                lock(&self.0).is_synced()
            }
            fn now_unix(&self) -> u64 {
                lock(&self.0).now_unix()
            }
            fn is_valid(&self) -> bool {
                lock(&self.0).is_valid()
            }
        }

        Self::from_backends(
            Box::new(SimPower::new()),
            Box::new(SimWifi::new()),
            Box::new(SimAudio::new()),
            Box::new(SimMic::new()),
            Box::new(SimImu::new()),
            Box::new(SimInput::new()),
            Box::new(SharedTime(time_shared)),
            Box::new(SimStorage::new()),
            Box::new(http),
        )
    }

    /// Initialize every backend that requires explicit setup.
    ///
    /// Idempotent and best-effort: errors are ignored so a missing optional
    /// peripheral never aborts boot. Call once at startup.
    pub fn init(&self) {
        let _ = self.power().init();
        let _ = self.wifi().init();
        let _ = self.storage().init();
    }

    /// Register an event listener callback for system hardware events.
    /// Returns a unique listener identifier that can be used to unregister it via [`remove_listener`].
    pub fn on_event(&self, listener: impl Fn(&SystemEvent) + Send + Sync + 'static) -> u64 {
        let id = self.next_listener_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u64;
        lock(&self.listeners).push((id, Arc::new(listener)));
        id
    }

    /// Unregisters an event listener by its ID. Returns true if the listener was found and removed.
    pub fn remove_listener(&self, id: u64) -> bool {
        let mut listeners = lock(&self.listeners);
        if let Some(pos) = listeners.iter().position(|(lid, _)| *lid == id) {
            listeners.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Broadcast an event to all registered listeners and queue it for synchronous consumers.
    pub fn emit_event(&self, event: SystemEvent) {
        let listeners: Vec<EventListener> = lock(&self.listeners)
            .iter()
            .map(|(_, l)| Arc::clone(l))
            .collect();
        for listener in &listeners {
            listener(&event);
        }

        let mut events = lock(&self.events);
        if events.len() >= MAX_EVENT_QUEUE_SIZE {
            events.pop_front();
        }
        events.push_back(event);
        self.event_condvar.notify_one();
    }

    /// Blocks the caller until a hardware/system event arrives or `timeout` expires.
    ///
    /// Consumes 0% CPU while blocked. Returns `None` if the timeout elapsed before an event arrived.
    pub fn wait_event(&self, timeout: Duration) -> Option<SystemEvent> {
        let mut events = lock(&self.events);
        if let Some(event) = events.pop_front() {
            return Some(event);
        }
        if timeout.is_zero() {
            return None;
        }

        let (mut guard, _) = self
            .event_condvar
            .wait_timeout(events, timeout)
            .unwrap_or_else(|p| p.into_inner());
        guard.pop_front()
    }

    /// Polls for the next available system hardware event without blocking.
    pub fn poll_event(&self) -> Option<SystemEvent> {
        self.wait_event(Duration::ZERO)
    }

    /// Lock the power/battery backend.
    pub fn power(&self) -> MutexGuard<'_, Box<dyn PowerBackend>> {
        lock(&self.power)
    }

    /// Reboot the system / device.
    pub fn reboot(&self) -> Result<(), crate::error::HalError> {
        self.power().reboot()
    }

    /// Lock the Wi-Fi backend.
    pub fn wifi(&self) -> MutexGuard<'_, Box<dyn WifiBackend>> {
        lock(&self.wifi)
    }

    /// Lock the audio backend.
    pub fn audio(&self) -> MutexGuard<'_, Box<dyn AudioBackend>> {
        lock(&self.audio)
    }

    /// Lock the microphone backend.
    pub fn mic(&self) -> MutexGuard<'_, Box<dyn MicBackend>> {
        lock(&self.mic)
    }

    /// Lock the IMU backend.
    pub fn imu(&self) -> MutexGuard<'_, Box<dyn ImuBackend>> {
        lock(&self.imu)
    }

    /// Lock the input backend.
    pub fn input(&self) -> MutexGuard<'_, Box<dyn InputBackend>> {
        lock(&self.input)
    }

    /// Lock the time/SNTP backend.
    pub fn time(&self) -> MutexGuard<'_, Box<dyn TimeBackend>> {
        lock(&self.time)
    }

    /// Lock the storage/SD-card backend.
    pub fn storage(&self) -> MutexGuard<'_, Box<dyn StorageBackend>> {
        lock(&self.storage)
    }

    /// Lock the HTTP/HTTPS client backend.
    pub fn http(&self) -> MutexGuard<'_, Box<dyn HttpBackend>> {
        lock(&self.http)
    }

    /// Turns display panel power on or off.
    pub fn set_display_power(&self, on: bool) -> Result<(), crate::error::HalError> {
        self.power().set_display_power(on)
    }

    /// Whether display panel is currently powered on.
    pub fn is_display_on(&self) -> bool {
        self.power().is_display_on().unwrap_or(true)
    }

    /// Sets display auto-screen-off inactivity timeout. `None` means never turn off automatically.
    pub fn set_screen_off_timeout(&self, timeout: Option<std::time::Duration>) -> Result<(), crate::error::HalError> {
        self.power().set_screen_off_timeout(timeout)
    }

    /// Inactivity timeout before display automatically turns off. `None` means never turn off.
    pub fn screen_off_timeout(&self) -> Option<std::time::Duration> {
        self.power().screen_off_timeout().unwrap_or(None)
    }

    /// Whether USB (VBUS) power is currently connected.
    pub fn is_usb_connected(&self) -> bool {
        self.power().is_usb_connected().unwrap_or(false)
    }

    /// Whether stay-awake on USB is enabled.
    pub fn stay_awake_on_usb(&self) -> bool {
        self.power().stay_awake_on_usb().unwrap_or(true)
    }

    /// Configures whether to stay awake when USB is connected.
    pub fn set_stay_awake_on_usb(&self, enable: bool) -> Result<(), crate::error::HalError> {
        self.power().set_stay_awake_on_usb(enable)
    }

    /// Access the thread-safe scheduled wakeup manager.
    pub fn wakeup_scheduler(&self) -> &crate::wakeup::WakeupScheduler {
        &self.wakeup
    }

    /// Advance every time-driven subsystem (Wi-Fi scan progress, audio EOF
    /// detection, input debouncing, storage polling, HTTP progress). Call periodically or once per UI frame.
    pub fn tick(&self) {
        self.wifi().tick();
        self.audio().tick();
        self.input().tick();
        self.storage().tick();
        self.http().tick();

        // Poll scheduled wakeup timer events
        for req in self.wakeup.poll_expired() {
            if req.wake_screen && !self.is_display_on() {
                let _ = self.set_display_power(true);
            }
            self.emit_event(SystemEvent::WakeupTriggered {
                id: req.id,
                wake_screen: req.wake_screen,
            });
        }

        if let Some(ev) = self.http().poll_event() {
            self.emit_event(ev);
        }

        #[cfg(not(target_os = "espidf"))]
        {
            // 1. Check user input actions (simulator)
            if let Some(action) = self.input().poll_action() {
                if !self.is_display_on() {
                    let _ = self.set_display_power(true);
                    // Swallow input action when waking up display
                } else {
                    self.emit_event(SystemEvent::InputAction(action));
                }
            }

            // 2. Check power changes
            let battery_opt = self.power().battery_percent().ok();
            let charging_opt = self.power().is_charging().ok();
            let voltage = self.power().battery_voltage_mv().unwrap_or(0);

            // 3. Check wifi status changes
            let wifi_status = self.wifi().status();

            // 4. Check SD card status changes
            let sd_status = self.storage().sd_status();

            let mut events_to_emit = Vec::new();
            {
                let mut cache = lock(&self.cache);
                if battery_opt.is_some()
                    && (cache.last_battery != battery_opt || cache.last_charging != charging_opt)
                {
                    cache.last_battery = battery_opt;
                    cache.last_charging = charging_opt;
                    if let Some(percent) = battery_opt {
                        events_to_emit.push(SystemEvent::BatteryChanged {
                            percent,
                            charging: charging_opt.unwrap_or(false),
                            voltage_mv: voltage,
                        });
                    }
                }

                if cache.last_wifi_status.as_ref() != Some(&wifi_status) {
                    cache.last_wifi_status = Some(wifi_status.clone());
                    events_to_emit.push(SystemEvent::WifiStatusChanged(wifi_status));
                }

                if cache.last_sd_status != Some(sd_status) {
                    cache.last_sd_status = Some(sd_status);
                    events_to_emit.push(SystemEvent::SdCardChanged(sd_status));
                }
            }

            for ev in events_to_emit {
                self.emit_event(ev);
            }
        }
    }
}

/// Lock a mutex, recovering the inner value if the lock was poisoned so a
/// panic in one backend never takes down the whole UI.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InputAction;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_board_event_queue_and_listeners() {
        let board = Arc::new(Board::simulated());
        let callback_count = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&callback_count);
        board.on_event(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(board.poll_event(), None);

        // Emit an event
        board.emit_event(SystemEvent::InputAction(InputAction::Back));

        // Both listener and queue should have received it
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
        let queued = board.wait_event(Duration::from_millis(50));
        assert_eq!(queued, Some(SystemEvent::InputAction(InputAction::Back)));

        // Queue is drained
        assert_eq!(board.poll_event(), None);
    }

    #[test]
    fn test_board_event_queue_capacity_bound() {
        let board = Board::simulated();

        // Emit 40 events (exceeds MAX_EVENT_QUEUE_SIZE = 32)
        for i in 0..40 {
            board.emit_event(SystemEvent::BatteryChanged {
                percent: i as u8,
                charging: false,
                voltage_mv: 4000,
            });
        }

        // The oldest 8 events (0..7) should have been dropped to protect memory
        let first = board.poll_event();
        assert_eq!(
            first,
            Some(SystemEvent::BatteryChanged {
                percent: 8,
                charging: false,
                voltage_mv: 4000,
            })
        );
    }
}

