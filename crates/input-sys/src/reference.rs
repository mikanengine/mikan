//! An independent Rust re-implementation of the C backend's normalization
//! rules — the test oracle.
//!
//! The C backend (`src/input_backend.c`) turns a raw device stream into a
//! normalized one. This module re-implements exactly those rules in Rust so
//! the two can be cross-checked bit for bit on fixed and randomized
//! sequences (`tests/cross_check.rs`). It is a test oracle, not duplication
//! for its own sake: it is never used at runtime, and if the C rules and
//! these rules drift apart the cross-check tests fail loudly, which is the
//! point.
//!
//! The rules, stated once here and enforced twice:
//!
//! - A press while the device is already down, and a release or repeat
//!   while it is not, are dropped (the device state is the truth, not the
//!   stream).
//! - A repeat while the key is down becomes a [`EVENT_KEY_REPEAT`] event.
//! - A trailing buffered [`EVENT_MOUSE_MOVE`] is overwritten by the next
//!   move (only the latest position matters); a trailing [`EVENT_WHEEL`] is
//!   summed (one summed event carries the same total).
//! - A zero wheel scroll is not an event.
//! - Repeating the current focus state is a no-op. Losing focus emits one
//!   release per held key and then button (in stored order), then
//!   [`EVENT_FOCUS_LOST`], and clears the device state.

use crate::{EVENT_MOUSE_MOVE, EVENT_WHEEL, InputEvent};

/// A reference normalizer: raw pushes in, normalized events out.
#[derive(Debug, Default)]
pub struct Normalizer {
    keys_down: Vec<u32>,
    buttons_down: Vec<u32>,
    focused: bool,
    events: Vec<InputEvent>,
}

impl Normalizer {
    /// Creates an empty normalizer with no device state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one key transition.
    pub fn push_key(&mut self, code: u32, pressed: bool, repeat: bool) {
        if pressed {
            let down = self.keys_down.contains(&code);
            if repeat {
                if down {
                    self.events.push(InputEvent::key_repeat(code));
                }
            } else if !down {
                self.keys_down.push(code);
                self.events.push(InputEvent::key(code, true));
            }
        } else if let Some(index) = self.keys_down.iter().position(|&held| held == code) {
            self.keys_down.remove(index);
            self.events.push(InputEvent::key(code, false));
        }
    }

    /// Feeds one mouse-button transition.
    pub fn push_mouse_button(&mut self, code: u32, pressed: bool) {
        if pressed {
            if !self.buttons_down.contains(&code) {
                self.buttons_down.push(code);
                self.events.push(InputEvent::mouse_button(code, true));
            }
        } else if let Some(index) = self.buttons_down.iter().position(|&held| held == code) {
            self.buttons_down.remove(index);
            self.events.push(InputEvent::mouse_button(code, false));
        }
    }

    /// Feeds one absolute cursor move; consecutive moves coalesce.
    pub fn push_mouse_move(&mut self, x: f64, y: f64) {
        self.coalesce(InputEvent::mouse_move(x, y), false);
    }

    /// Feeds one wheel scroll; consecutive scrolls coalesce by summing.
    pub fn push_wheel(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        self.coalesce(InputEvent::wheel(dx, dy), true);
    }

    /// Feeds one focus change.
    pub fn push_focus(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if focused {
            self.events.push(InputEvent::focus(true));
            return;
        }
        for &code in &self.keys_down {
            self.events.push(InputEvent::key(code, false));
        }
        for &code in &self.buttons_down {
            self.events.push(InputEvent::mouse_button(code, false));
        }
        self.keys_down.clear();
        self.buttons_down.clear();
        self.events.push(InputEvent::focus(false));
    }

    /// The normalized events emitted so far, in stream order.
    #[must_use]
    pub fn events(&self) -> &[InputEvent] {
        &self.events
    }

    /// Whether the key is down in the reference's device state.
    #[must_use]
    pub fn is_key_down(&self, code: u32) -> bool {
        self.keys_down.contains(&code)
    }

    /// Whether the mouse button is down in the reference's device state.
    #[must_use]
    pub fn is_mouse_button_down(&self, code: u32) -> bool {
        self.buttons_down.contains(&code)
    }

    /// Every event emitted since the last `take`, in stream order.
    pub fn take(&mut self) -> Vec<InputEvent> {
        core::mem::take(&mut self.events)
    }

    /// Appends `event`, coalescing into a trailing event of the same kind.
    /// `combine` sums the payload; otherwise it overwrites.
    fn coalesce(&mut self, event: InputEvent, combine: bool) {
        if let Some(last) = self.events.last_mut()
            && last.kind == event.kind
            && (event.kind == EVENT_MOUSE_MOVE || event.kind == EVENT_WHEEL)
        {
            if combine {
                last.x += event.x;
                last.y += event.y;
            } else {
                last.x = event.x;
                last.y = event.y;
            }
            return;
        }
        self.events.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_and_spurious_releases_are_dropped() {
        let mut reference = Normalizer::new();
        reference.push_key(crate::keys::KEY_A, true, false);
        reference.push_key(crate::keys::KEY_A, true, false);
        reference.push_key(crate::keys::KEY_A, false, false);
        reference.push_key(crate::keys::KEY_A, false, false);
        reference.push_mouse_button(crate::MOUSE_LEFT, true);
        reference.push_mouse_button(crate::MOUSE_LEFT, true);
        reference.push_mouse_button(crate::MOUSE_LEFT, false);
        reference.push_mouse_button(crate::MOUSE_LEFT, false);
        assert_eq!(
            reference.events(),
            &[
                InputEvent::key(crate::keys::KEY_A, true),
                InputEvent::key(crate::keys::KEY_A, false),
                InputEvent::mouse_button(crate::MOUSE_LEFT, true),
                InputEvent::mouse_button(crate::MOUSE_LEFT, false),
            ]
        );
    }

    #[test]
    fn focus_loss_releases_everything_then_sweeps() {
        let mut reference = Normalizer::new();
        reference.push_focus(true);
        reference.push_key(crate::keys::KEY_W, true, false);
        reference.push_mouse_button(crate::MOUSE_RIGHT, true);
        reference.push_focus(false);
        reference.push_focus(false);
        assert_eq!(
            reference.events(),
            &[
                InputEvent::focus(true),
                InputEvent::key(crate::keys::KEY_W, true),
                InputEvent::mouse_button(crate::MOUSE_RIGHT, true),
                InputEvent::key(crate::keys::KEY_W, false),
                InputEvent::mouse_button(crate::MOUSE_RIGHT, false),
                InputEvent::focus(false),
            ]
        );
    }

    #[test]
    fn moves_and_wheels_coalesce() {
        let mut reference = Normalizer::new();
        reference.push_mouse_move(1.0, 1.0);
        reference.push_mouse_move(2.0, 2.0);
        reference.push_wheel(1.0, 0.0);
        reference.push_wheel(0.0, -2.0);
        reference.push_wheel(0.0, 0.0);
        reference.push_mouse_button(crate::MOUSE_LEFT, true);
        reference.push_mouse_move(3.0, 3.0);
        assert_eq!(
            reference.events(),
            &[
                InputEvent::mouse_move(2.0, 2.0),
                InputEvent::wheel(1.0, -2.0),
                InputEvent::mouse_button(crate::MOUSE_LEFT, true),
                InputEvent::mouse_move(3.0, 3.0),
            ]
        );
    }

    #[test]
    fn repeats_only_fire_while_held() {
        let mut reference = Normalizer::new();
        reference.push_key(crate::keys::KEY_D, true, false);
        reference.push_key(crate::keys::KEY_D, true, true);
        reference.push_key(crate::keys::KEY_D, false, false);
        reference.push_key(crate::keys::KEY_D, true, true);
        assert_eq!(
            reference.events(),
            &[
                InputEvent::key(crate::keys::KEY_D, true),
                InputEvent::key_repeat(crate::keys::KEY_D),
                InputEvent::key(crate::keys::KEY_D, false),
            ]
        );
    }
}
