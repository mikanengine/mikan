//! Device-code mapping: winit taxonomy to the native module's USB HID codes.
//!
//! The native backend has no winit dependency, so [`mikan_input`] translates
//! once, at every platform event: a [`PhysicalKey`] becomes the HID usage id
//! ([`key_code`]), a [`MouseButton`] becomes a stable button code
//! ([`mouse_button`]). winit 0.30's `KeyCode` carries no raw value, so the
//! translation is the table below; it covers every key the engine names, and
//! anything else resolves to key code `0`, which the native module tracks in
//! raw state but refuses to bind.

use winit::event::MouseButton;
use winit::keyboard::{KeyCode, PhysicalKey};

use mikan_input_sys as inf;
use mikan_input_sys::keys;

/// Translates a physical key to its HID usage id, or [`keys::KEY_UNKNOWN`].
#[must_use]
pub fn key_code(key: PhysicalKey) -> u32 {
    match key {
        PhysicalKey::Code(code) => match code {
            KeyCode::KeyA => keys::KEY_A,
            KeyCode::KeyB => keys::KEY_B,
            KeyCode::KeyC => keys::KEY_C,
            KeyCode::KeyD => keys::KEY_D,
            KeyCode::KeyE => keys::KEY_E,
            KeyCode::KeyF => keys::KEY_F,
            KeyCode::KeyG => keys::KEY_G,
            KeyCode::KeyH => keys::KEY_H,
            KeyCode::KeyI => keys::KEY_I,
            KeyCode::KeyJ => keys::KEY_J,
            KeyCode::KeyK => keys::KEY_K,
            KeyCode::KeyL => keys::KEY_L,
            KeyCode::KeyM => keys::KEY_M,
            KeyCode::KeyN => keys::KEY_N,
            KeyCode::KeyO => keys::KEY_O,
            KeyCode::KeyP => keys::KEY_P,
            KeyCode::KeyQ => keys::KEY_Q,
            KeyCode::KeyR => keys::KEY_R,
            KeyCode::KeyS => keys::KEY_S,
            KeyCode::KeyT => keys::KEY_T,
            KeyCode::KeyU => keys::KEY_U,
            KeyCode::KeyV => keys::KEY_V,
            KeyCode::KeyW => keys::KEY_W,
            KeyCode::KeyX => keys::KEY_X,
            KeyCode::KeyY => keys::KEY_Y,
            KeyCode::KeyZ => keys::KEY_Z,
            KeyCode::Digit0 => keys::KEY_DIGIT0,
            KeyCode::Digit1 => keys::KEY_DIGIT1,
            KeyCode::Digit2 => keys::KEY_DIGIT2,
            KeyCode::Digit3 => keys::KEY_DIGIT3,
            KeyCode::Digit4 => keys::KEY_DIGIT4,
            KeyCode::Digit5 => keys::KEY_DIGIT5,
            KeyCode::Digit6 => keys::KEY_DIGIT6,
            KeyCode::Digit7 => keys::KEY_DIGIT7,
            KeyCode::Digit8 => keys::KEY_DIGIT8,
            KeyCode::Digit9 => keys::KEY_DIGIT9,
            KeyCode::Enter => keys::KEY_ENTER,
            KeyCode::Escape => keys::KEY_ESCAPE,
            KeyCode::Backspace => keys::KEY_BACKSPACE,
            KeyCode::Tab => keys::KEY_TAB,
            KeyCode::Space => keys::KEY_SPACE,
            KeyCode::Minus => keys::KEY_MINUS,
            KeyCode::Equal => keys::KEY_EQUAL,
            KeyCode::BracketLeft => keys::KEY_BRACKETLEFT,
            KeyCode::BracketRight => keys::KEY_BRACKETRIGHT,
            KeyCode::Backslash => keys::KEY_BACKSLASH,
            KeyCode::Semicolon => keys::KEY_SEMICOLON,
            KeyCode::Quote => keys::KEY_QUOTE,
            KeyCode::Backquote => keys::KEY_BACKQUOTE,
            KeyCode::Comma => keys::KEY_COMMA,
            KeyCode::Period => keys::KEY_PERIOD,
            KeyCode::Slash => keys::KEY_SLASH,
            KeyCode::CapsLock => keys::KEY_CAPSLOCK,
            KeyCode::F1 => keys::KEY_F1,
            KeyCode::F2 => keys::KEY_F2,
            KeyCode::F3 => keys::KEY_F3,
            KeyCode::F4 => keys::KEY_F4,
            KeyCode::F5 => keys::KEY_F5,
            KeyCode::F6 => keys::KEY_F6,
            KeyCode::F7 => keys::KEY_F7,
            KeyCode::F8 => keys::KEY_F8,
            KeyCode::F9 => keys::KEY_F9,
            KeyCode::F10 => keys::KEY_F10,
            KeyCode::F11 => keys::KEY_F11,
            KeyCode::F12 => keys::KEY_F12,
            KeyCode::Insert => keys::KEY_INSERT,
            KeyCode::Home => keys::KEY_HOME,
            KeyCode::PageUp => keys::KEY_PAGEUP,
            KeyCode::Delete => keys::KEY_DELETE,
            KeyCode::End => keys::KEY_END,
            KeyCode::PageDown => keys::KEY_PAGEDOWN,
            KeyCode::ArrowRight => keys::KEY_ARROWRIGHT,
            KeyCode::ArrowLeft => keys::KEY_ARROWLEFT,
            KeyCode::ArrowDown => keys::KEY_ARROWDOWN,
            KeyCode::ArrowUp => keys::KEY_ARROWUP,
            KeyCode::ControlLeft => keys::KEY_CONTROLLEFT,
            KeyCode::ShiftLeft => keys::KEY_SHIFTLEFT,
            KeyCode::AltLeft => keys::KEY_ALTLEFT,
            KeyCode::SuperLeft => keys::KEY_METALEFT,
            KeyCode::ControlRight => keys::KEY_CONTROLRIGHT,
            KeyCode::ShiftRight => keys::KEY_SHIFTRIGHT,
            KeyCode::AltRight => keys::KEY_ALTRIGHT,
            KeyCode::SuperRight => keys::KEY_METARIGHT,
            _ => keys::KEY_UNKNOWN,
        },
        PhysicalKey::Unidentified(_) => keys::KEY_UNKNOWN,
    }
}

/// Translates a mouse button to its stable button code.
///
/// The table matches the native module: primary 1, secondary 2, middle 3,
/// and every other button `256` plus its `u16` id (`Back` and `Forward`
/// therefore overlap whatever `Other` values a device produces, which is
/// harmless: still distinct raw state, still distinct code).
#[must_use]
pub fn mouse_button(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => inf::MOUSE_LEFT,
        MouseButton::Right => inf::MOUSE_RIGHT,
        MouseButton::Middle => inf::MOUSE_MIDDLE,
        MouseButton::Back => inf::MOUSE_OTHER_BASE,
        MouseButton::Forward => inf::MOUSE_OTHER_BASE + 1,
        MouseButton::Other(id) => inf::MOUSE_OTHER_BASE + u32::from(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_numbers_and_modifiers_map_to_hid_ids() {
        assert_eq!(key_code(PhysicalKey::Code(KeyCode::KeyD)), keys::KEY_D);
        assert_eq!(key_code(PhysicalKey::Code(KeyCode::KeyJ)), keys::KEY_J);
        assert_eq!(key_code(PhysicalKey::Code(KeyCode::KeyW)), keys::KEY_W);
        assert_eq!(
            key_code(PhysicalKey::Code(KeyCode::Digit1)),
            keys::KEY_DIGIT1
        );
        assert_eq!(
            key_code(PhysicalKey::Code(KeyCode::Escape)),
            keys::KEY_ESCAPE
        );
        assert_eq!(key_code(PhysicalKey::Code(KeyCode::F1)), keys::KEY_F1);
        assert_eq!(
            key_code(PhysicalKey::Code(KeyCode::ArrowLeft)),
            keys::KEY_ARROWLEFT
        );
        assert_eq!(
            key_code(PhysicalKey::Code(KeyCode::ShiftLeft)),
            keys::KEY_SHIFTLEFT
        );
    }

    #[test]
    fn unnamable_keys_map_to_the_reserved_code() {
        for key in [
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
            // Numpad keys are outside the engine's calibrated table.
            PhysicalKey::Code(KeyCode::NumpadAdd),
        ] {
            assert_eq!(key_code(key), keys::KEY_UNKNOWN);
        }
    }

    #[test]
    fn mouse_buttons_map_into_the_other_range() {
        assert_eq!(mouse_button(MouseButton::Left), 1);
        assert_eq!(mouse_button(MouseButton::Right), 2);
        assert_eq!(mouse_button(MouseButton::Middle), 3);
        assert_eq!(mouse_button(MouseButton::Back), inf::MOUSE_OTHER_BASE);
        assert_eq!(
            mouse_button(MouseButton::Other(7)),
            inf::MOUSE_OTHER_BASE + 7
        );
    }
}
