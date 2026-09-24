//! Coalesce native input; never queue a second toggle behind a slow first one.
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(crate) struct ToggleInput {
    pressed: AtomicBool,
    pending: AtomicBool,
}

impl ToggleInput {
    pub(crate) const fn new() -> Self {
        Self { pressed: AtomicBool::new(false), pending: AtomicBool::new(false) }
    }

    pub(crate) fn key_event(&self, pressed: bool) -> bool {
        let was_pressed = self.pressed.swap(pressed, Ordering::SeqCst);
        pressed && !was_pressed
    }

    pub(crate) fn acquire(&self) -> Option<TogglePermit<'_>> {
        self.pending.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok().map(|_| TogglePermit(self))
    }
}

pub(crate) struct TogglePermit<'a>(&'a ToggleInput);

impl Drop for TogglePermit<'_> {
    fn drop(&mut self) { self.0.pending.store(false, Ordering::SeqCst); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_key_does_not_toggle_repeatedly() {
        let input = ToggleInput::new();
        assert!(input.key_event(true));
        assert!(!input.key_event(true));
        assert!(!input.key_event(false));
        assert!(input.key_event(true));
    }

    #[test]
    fn pending_toggle_is_coalesced_and_released_on_drop() {
        let input = ToggleInput::new();
        let permit = input.acquire().unwrap();
        assert!(input.acquire().is_none());
        drop(permit);
        assert!(input.acquire().is_some());
    }
}
