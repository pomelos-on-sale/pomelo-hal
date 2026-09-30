use std::collections::VecDeque;

use crate::traits::InputBackend;
use crate::types::InputAction;

/// Desktop simulator input backend.
pub struct SimInput {
    queue: VecDeque<InputAction>,
}

impl SimInput {
    /// Create a new, empty simulator input backend.
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

impl Default for SimInput {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBackend for SimInput {
    fn poll_action(&mut self) -> Option<InputAction> {
        self.queue.pop_front()
    }

    fn push_action(&mut self, action: InputAction) {
        self.queue.push_back(action);
    }
}
