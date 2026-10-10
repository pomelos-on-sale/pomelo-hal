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
    AudioBackend, ImuBackend, InputBackend, MicBackend, PowerBackend, TimeBackend, WifiBackend,
};
use crate::types::{SystemEvent, WifiStatus};

pub type EventListener = Arc<dyn Fn(&SystemEvent) + Send + Sync>;

const MAX_EVENT_QUEUE_SIZE: usize = 32;

#[allow(dead_code)]
#[derive(Default)]
struct EventStateCache {
    last_battery: Option<u8>,
    last_charging: Option<bool>,
    last_wifi_status: Option<WifiStatus>,
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
    listeners: Mutex<Vec<EventListener>>,
    events: Mutex<VecDeque<SystemEvent>>,
    event_condvar: Condvar,
    #[allow(dead_code)]
    cache: Mutex<EventStateCache>,
}

impl Board {
    /// Assemble a board from one backend per subsystem.
    ///
    /// The order is `power`, `wifi`, `audio`, `mic`, `imu`, `input` — the same order the accessors
    /// appear in. It takes boxes rather than generics so the caller can mix concrete types
    /// and, in a test, its own fakes.
    pub fn from_backends(
        power: Box<dyn PowerBackend>,
        wifi: Box<dyn WifiBackend>,
        audio: Box<dyn AudioBackend>,
        mic: Box<dyn MicBackend>,
        imu: Box<dyn ImuBackend>,
        input: Box<dyn InputBackend>,
        time: Box<dyn TimeBackend>,
    ) -> Self {
        Self {
            power: Mutex::new(power),
            wifi: Mutex::new(wifi),
            audio: Mutex::new(audio),
            mic: Mutex::new(mic),
            imu: Mutex::new(imu),
            input: Mutex::new(input),
            time: Mutex::new(time),
            listeners: Mutex::new(Vec::new()),
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENT_QUEUE_SIZE)),
            event_condvar: Condvar::new(),
            cache: Mutex::new(EventStateCache::default()),
        }
    }

    /// A board of desktop simulator backends.
    #[cfg(not(target_os = "espidf"))]
    pub fn simulated() -> Self {
        use crate::sim::{SimAudio, SimImu, SimInput, SimMic, SimPower, SimTime, SimWifi};
        Self::from_backends(
            Box::new(SimPower::new()),
            Box::new(SimWifi::new()),
            Box::new(SimAudio::new()),
            Box::new(SimMic::new()),
            Box::new(SimImu::new()),
            Box::new(SimInput::new()),
            Box::new(SimTime::new()),
        )
    }

    /// Initialize every backend that requires explicit setup.
    ///
    /// Idempotent and best-effort: errors are ignored so a missing optional
    /// peripheral never aborts boot. Call once at startup.
    pub fn init(&self) {
        let _ = self.power().init();
        let _ = self.wifi().init();
    }

    /// Register an event listener callback for system hardware events.
    pub fn on_event(&self, listener: impl Fn(&SystemEvent) + Send + Sync + 'static) {
        lock(&self.listeners).push(Arc::new(listener));
    }

    /// Broadcast an event to all registered listeners and queue it for synchronous consumers.
    pub fn emit_event(&self, event: SystemEvent) {
        let listeners: Vec<EventListener> = lock(&self.listeners).clone();
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

    /// Advance every time-driven subsystem (Wi-Fi scan progress, audio EOF
    /// detection, input debouncing). Call periodically or once per UI frame.
    pub fn tick(&self) {
        self.wifi().tick();
        self.audio().tick();
        self.input().tick();

        #[cfg(not(target_os = "espidf"))]
        {
            // 1. Check user input actions (simulator)
            if let Some(action) = self.input().poll_action() {
                self.emit_event(SystemEvent::InputAction(action));
            }

            // 2. Check power changes
            let battery_opt = self.power().battery_percent().ok();
            let charging_opt = self.power().is_charging().ok();
            let voltage = self.power().battery_voltage_mv().unwrap_or(0);

            // 3. Check wifi status changes
            let wifi_status = self.wifi().status();

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

