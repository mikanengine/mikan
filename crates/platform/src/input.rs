//! Keyboard and mouse state for the current frame.
//!
//! The platform runner translates window events into calls on [`InputState`].
//! Applications only read from it: the state is written by exactly one place,
//! which keeps the "who owns input" question trivial.
//!
//! Alongside the state, the runner records every device event it receives
//! into one per-frame [`DeviceEvent`] log (see
//! [`take_events`](InputState::take_events)). The log is what the action
//! mapping layer (`mikan-input`) consumes: it streams the window system's
//! device events — including repeats and wheel scrolls, which held-state
//! queries cannot express — to whatever code wants to process them, without
//! the platform layer knowing about action mapping.

use std::collections::HashSet;

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::PhysicalKey;

/// One device event the window system produced this frame, in arrival order.
///
/// The events are the raw stream, not the [state](InputState): repeats,
/// duplicate presses, and wheel scrolls appear exactly as the window system
/// produced them. Everything downstream normalizes what it needs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceEvent {
    /// A key transition. `repeat` marks an operating-system key repeat.
    Key {
        /// The physical key, layout independent.
        key: PhysicalKey,
        /// Whether the key went down (`false` means it came up).
        pressed: bool,
        /// Whether this press is a repeat of a held key.
        repeat: bool,
    },
    /// A mouse-button transition.
    MouseButton {
        /// Which button.
        button: MouseButton,
        /// Whether it went down (`false` means it came up).
        pressed: bool,
    },
    /// The cursor is at an absolute physical-pixel position.
    CursorMoved {
        /// The position in physical pixels.
        position: PhysicalPosition<f64>,
    },
    /// The wheel scrolled. The delta is normalized to pixels: line-based
    /// scrolling is scaled by the conventional 24 pixels per line, pixel-
    /// based scrolling passes through.
    MouseWheel {
        /// The `(x, y)` scroll delta.
        delta: (f64, f64),
    },
    /// The window's keyboard focus changed.
    FocusChanged {
        /// Whether the window is now focused.
        focused: bool,
    },
}

/// How many device events one frame's log holds before dropping the newest.
///
/// A full log means more than [`MAX_FRAME_EVENTS`] device events arrived in
/// one frame, which is not a stream a game can meaningfully replay. The
/// dropped count [`InputState::dropped_events`] reports how many.
const MAX_FRAME_EVENTS: usize = 1024;

/// Snapshot of the input devices for the duration of one frame.
///
/// Two kinds of query are available:
///
/// - *held* state (`is_key_pressed`) stays true for as long as the key is down.
/// - *edge* state (`is_key_just_pressed`, `is_key_just_released`) is true only
///   for the frame in which the transition happened, and is cleared by
///   [`end_frame`](InputState::end_frame).
///
/// Keyboard state is tracked by physical key, so it is layout independent.
///
/// The per-frame [`DeviceEvent`] log lives beside this: [`take_events`]
/// drains it for a step that wants the raw stream, and [`end_frame`] clears
/// exactly what is left over when it clears the edge state.
///
/// [`take_events`]: InputState::take_events
#[derive(Debug, Default)]
pub struct InputState {
    keys: HashSet<PhysicalKey>,
    keys_just_pressed: HashSet<PhysicalKey>,
    keys_just_released: HashSet<PhysicalKey>,
    mouse_buttons: HashSet<MouseButton>,
    mouse_just_pressed: HashSet<MouseButton>,
    mouse_just_released: HashSet<MouseButton>,
    cursor: Option<PhysicalPosition<f64>>,
    cursor_delta: (f64, f64),
    wheel_delta: (f64, f64),
    focused: bool,
    events: Vec<DeviceEvent>,
    dropped_events: usize,
}

impl InputState {
    /// Creates an empty input snapshot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` while `key` is held down.
    #[must_use]
    pub fn is_key_pressed(&self, key: PhysicalKey) -> bool {
        self.keys.contains(&key)
    }

    /// Returns `true` only during the frame in which `key` went down.
    #[must_use]
    pub fn is_key_just_pressed(&self, key: PhysicalKey) -> bool {
        self.keys_just_pressed.contains(&key)
    }

    /// Returns `true` only during the frame in which `key` was released.
    #[must_use]
    pub fn is_key_just_released(&self, key: PhysicalKey) -> bool {
        self.keys_just_released.contains(&key)
    }

    /// Returns `true` while `button` is held down.
    #[must_use]
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_buttons.contains(&button)
    }

    /// Returns `true` only during the frame in which `button` went down.
    #[must_use]
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed.contains(&button)
    }

    /// Returns `true` only during the frame in which `button` was released.
    #[must_use]
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released.contains(&button)
    }

    /// Cursor position in physical pixels, or `None` if the cursor has not been
    /// seen inside the window yet.
    #[must_use]
    pub fn cursor_position(&self) -> Option<PhysicalPosition<f64>> {
        self.cursor
    }

    /// Cumulative cursor movement since the last [`end_frame`](Self::end_frame),
    /// in physical pixels.
    #[must_use]
    pub fn cursor_delta(&self) -> (f64, f64) {
        self.cursor_delta
    }

    /// Cumulative wheel scroll since the last [`end_frame`](Self::end_frame),
    /// in pixels: `(x, y)`.
    #[must_use]
    pub fn wheel_delta(&self) -> (f64, f64) {
        self.wheel_delta
    }

    /// How many device events were dropped because one frame's log overflowed.
    ///
    /// Overflow means more than [`MAX_FRAME_EVENTS`] device events arrived in
    /// one frame, which no gameplay loop replays meaningfully; everything
    /// before the overflow is still in the log.
    #[must_use]
    pub fn dropped_events(&self) -> usize {
        self.dropped_events
    }

    /// Whether the window currently has keyboard focus.
    #[must_use]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Drains the frame's recorded device events into `out`, which is
    /// cleared first and extended in arrival order.
    ///
    /// Callers should drain once per fixed step: the first step of a frame
    /// receives everything accumulated since the last drained step, later
    /// steps receive nothing, and a frame that runs no fixed step keeps its
    /// events for the next one — exactly the visibility contract the press/
    /// release edges follow. [`end_frame`](Self::end_frame) clears whatever
    /// a step did not take.
    pub fn take_events(&mut self, out: &mut Vec<DeviceEvent>) {
        out.clear();
        out.append(&mut self.events);
    }

    /// Records one device event in the frame's log.
    fn record(&mut self, event: DeviceEvent) {
        if self.events.len() == MAX_FRAME_EVENTS {
            self.dropped_events += 1;
            return;
        }
        self.events.push(event);
    }

    /// Records a key transition.
    ///
    /// `repeat` marks a key repeat generated by the operating system; repeats do
    /// not produce a new "just pressed" edge.
    pub fn handle_key(&mut self, key: PhysicalKey, state: ElementState, repeat: bool) {
        let pressed = state == ElementState::Pressed;
        self.record(DeviceEvent::Key {
            key,
            pressed,
            repeat,
        });
        match state {
            ElementState::Pressed => {
                let first_press = self.keys.insert(key);
                if first_press && !repeat {
                    self.keys_just_pressed.insert(key);
                }
            }
            ElementState::Released => {
                if self.keys.remove(&key) {
                    self.keys_just_released.insert(key);
                }
            }
        }
    }

    /// Records a mouse button transition.
    pub fn handle_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let pressed = state == ElementState::Pressed;
        self.record(DeviceEvent::MouseButton { button, pressed });
        match state {
            ElementState::Pressed => {
                if self.mouse_buttons.insert(button) {
                    self.mouse_just_pressed.insert(button);
                }
            }
            ElementState::Released => {
                if self.mouse_buttons.remove(&button) {
                    self.mouse_just_released.insert(button);
                }
            }
        }
    }

    /// Records a cursor movement, accumulating the delta since the last frame.
    pub fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.record(DeviceEvent::CursorMoved { position });
        if let Some(previous) = self.cursor {
            self.cursor_delta.0 += position.x - previous.x;
            self.cursor_delta.1 += position.y - previous.y;
        }
        self.cursor = Some(position);
    }

    /// Records a wheel scroll, accumulating the pixel delta since the last
    /// frame. Line-based scrolling is converted to the conventional 24
    /// pixels per line, pixel-based scrolling passes through.
    pub fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (f64::from(x) * 24.0, f64::from(y) * 24.0),
            MouseScrollDelta::PixelDelta(position) => (position.x, position.y),
        };
        self.record(DeviceEvent::MouseWheel { delta: (dx, dy) });
        self.wheel_delta.0 += dx;
        self.wheel_delta.1 += dy;
    }

    /// Records a focus change. Losing focus releases every held key and button,
    /// so a key cannot remain "stuck down" while the window is in the background.
    pub fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.record(DeviceEvent::FocusChanged { focused });
        }
        self.focused = focused;
        if !focused {
            self.release_all();
        }
    }

    /// Releases every held key and button, reporting each as a release edge.
    pub fn release_all(&mut self) {
        for key in core::mem::take(&mut self.keys) {
            self.keys_just_released.insert(key);
        }
        for button in core::mem::take(&mut self.mouse_buttons) {
            self.mouse_just_released.insert(button);
        }
    }

    /// Clears per-frame edge state and the accumulated cursor and wheel
    /// deltas.
    ///
    /// The runner calls this once per frame, after the application has been
    /// updated and rendered. Whatever device events a step did not
    /// [`take`](Self::take_events) are cleared with the rest.
    pub fn end_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
        self.cursor_delta = (0.0, 0.0);
        self.wheel_delta = (0.0, 0.0);
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    const KEY_A: PhysicalKey = PhysicalKey::Code(KeyCode::KeyA);
    const KEY_D: PhysicalKey = PhysicalKey::Code(KeyCode::KeyD);

    #[test]
    fn press_then_release_produces_the_expected_edges() {
        let mut input = InputState::new();
        input.handle_key(KEY_A, ElementState::Pressed, false);
        assert!(input.is_key_pressed(KEY_A));
        assert!(input.is_key_just_pressed(KEY_A));

        input.end_frame();
        assert!(input.is_key_pressed(KEY_A), "held state survives the frame");
        assert!(!input.is_key_just_pressed(KEY_A), "edge state is per frame");

        input.handle_key(KEY_A, ElementState::Released, false);
        assert!(!input.is_key_pressed(KEY_A));
        assert!(input.is_key_just_released(KEY_A));
    }

    #[test]
    fn key_repeats_do_not_re_trigger_the_press_edge() {
        let mut input = InputState::new();
        input.handle_key(KEY_D, ElementState::Pressed, false);
        input.end_frame();
        input.handle_key(KEY_D, ElementState::Pressed, true);
        assert!(input.is_key_pressed(KEY_D));
        assert!(!input.is_key_just_pressed(KEY_D));
    }

    #[test]
    fn release_without_press_is_ignored() {
        let mut input = InputState::new();
        input.handle_key(KEY_A, ElementState::Released, false);
        assert!(!input.is_key_just_released(KEY_A));
    }

    #[test]
    fn losing_focus_releases_every_held_key() {
        let mut input = InputState::new();
        input.handle_key(KEY_A, ElementState::Pressed, false);
        input.handle_key(KEY_D, ElementState::Pressed, false);
        input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
        input.end_frame();

        input.set_focused(false);

        assert!(!input.is_focused());
        assert!(!input.is_key_pressed(KEY_A));
        assert!(!input.is_key_pressed(KEY_D));
        assert!(!input.is_mouse_button_pressed(MouseButton::Left));
        assert!(input.is_key_just_released(KEY_A));
        assert!(input.is_key_just_released(KEY_D));
        assert!(input.is_mouse_button_just_released(MouseButton::Left));
    }

    #[test]
    fn cursor_delta_accumulates_within_a_frame_and_resets_between_frames() {
        let mut input = InputState::new();
        input.handle_cursor_moved(PhysicalPosition::new(10.0, 10.0));
        assert_eq!(
            input.cursor_delta(),
            (0.0, 0.0),
            "first sighting has no delta"
        );

        input.handle_cursor_moved(PhysicalPosition::new(15.0, 7.0));
        assert_eq!(input.cursor_delta(), (5.0, -3.0));
        assert_eq!(
            input.cursor_position(),
            Some(PhysicalPosition::new(15.0, 7.0))
        );

        input.end_frame();
        assert_eq!(input.cursor_delta(), (0.0, 0.0));
        assert_eq!(
            input.cursor_position(),
            Some(PhysicalPosition::new(15.0, 7.0))
        );
    }

    #[test]
    fn mouse_buttons_track_edges_like_keys() {
        let mut input = InputState::new();
        input.handle_mouse_button(MouseButton::Right, ElementState::Pressed);
        assert!(input.is_mouse_button_pressed(MouseButton::Right));
        assert!(input.is_mouse_button_just_pressed(MouseButton::Right));
        input.end_frame();
        assert!(input.is_mouse_button_pressed(MouseButton::Right));
        assert!(!input.is_mouse_button_just_pressed(MouseButton::Right));
    }

    #[test]
    fn the_event_log_records_the_raw_stream_in_order() {
        let mut input = InputState::new();
        input.handle_key(KEY_D, ElementState::Pressed, false);
        input.handle_key(KEY_D, ElementState::Pressed, true);
        input.handle_mouse_button(MouseButton::Left, ElementState::Pressed);
        input.handle_cursor_moved(PhysicalPosition::new(5.0, 6.0));
        input.handle_mouse_wheel(MouseScrollDelta::LineDelta(1.0, -2.0));
        input.set_focused(true);
        input.set_focused(false);

        let mut events = Vec::new();
        input.take_events(&mut events);
        assert_eq!(
            events,
            vec![
                DeviceEvent::Key {
                    key: KEY_D,
                    pressed: true,
                    repeat: false
                },
                DeviceEvent::Key {
                    key: KEY_D,
                    pressed: true,
                    repeat: true
                },
                DeviceEvent::MouseButton {
                    button: MouseButton::Left,
                    pressed: true
                },
                DeviceEvent::CursorMoved {
                    position: PhysicalPosition::new(5.0, 6.0)
                },
                DeviceEvent::MouseWheel {
                    delta: (24.0, -48.0)
                },
                DeviceEvent::FocusChanged { focused: true },
                DeviceEvent::FocusChanged { focused: false },
            ]
        );
    }

    #[test]
    fn taking_the_log_leaves_it_empty_but_end_frame_clears_the_rest() {
        let mut input = InputState::new();
        input.handle_key(KEY_A, ElementState::Pressed, false);
        let mut out = Vec::new();
        input.take_events(&mut out);
        assert_eq!(out.len(), 1);
        input.handle_key(KEY_D, ElementState::Pressed, false);
        input.end_frame();
        input.take_events(&mut out);
        assert!(
            out.is_empty(),
            "events a step did not take are cleared with the edges"
        );
    }

    #[test]
    fn wheel_deltas_line_and_pixel_are_normalized_to_pixels() {
        let mut input = InputState::new();
        input.handle_mouse_wheel(MouseScrollDelta::LineDelta(1.0, 2.0));
        assert_eq!(input.wheel_delta(), (24.0, 48.0));
        input.handle_mouse_wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            -3.0, 0.5,
        )));
        assert_eq!(
            input.wheel_delta(),
            (21.0, 48.5),
            "deltas accumulate within a frame"
        );
        assert_eq!(
            input.dropped_events(),
            0,
            "an ordinary frame never overflows"
        );
        input.end_frame();
        assert_eq!(input.wheel_delta(), (0.0, 0.0));
    }

    #[test]
    fn focus_changes_log_only_on_the_differences() {
        let mut input = InputState::new();
        input.set_focused(true);
        input.set_focused(true);
        let mut events = Vec::new();
        input.take_events(&mut events);
        assert_eq!(events, vec![DeviceEvent::FocusChanged { focused: true }]);
    }

    #[test]
    fn an_overflowing_log_drops_the_newest_and_counts() {
        let mut input = InputState::new();
        for _ in 0..MAX_FRAME_EVENTS {
            input.handle_cursor_moved(PhysicalPosition::new(0.0, 0.0));
        }
        assert_eq!(input.dropped_events(), 0);
        input.handle_cursor_moved(PhysicalPosition::new(1.0, 1.0));
        assert_eq!(input.dropped_events(), 1);

        let mut events = Vec::new();
        input.take_events(&mut events);
        assert_eq!(events.len(), MAX_FRAME_EVENTS, "what fit is still recorded");
    }
}
