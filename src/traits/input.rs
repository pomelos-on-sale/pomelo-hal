//! User input hardware trait — physical buttons, gestures, and navigation.

use crate::types::InputAction;

/// Hardware abstraction for system input actions (buttons, touch gestures).
pub trait InputBackend: Send + Sync {
    /// Poll the next input action, if any is pending.
    fn poll_action(&mut self) -> Option<InputAction> {
        None
    }

    /// Push an input action into the backend.
    ///
    /// Primarily used by simulators, desktop test benches, or software event injection.
    fn push_action(&mut self, _action: InputAction) {}

    /// Advance time-driven input state (debouncing, gesture timers, etc.).
    fn tick(&mut self) {}
}
