//! Key codes, mirroring the `MIKAN_INPUT_KEY_*` constants in
//! `src/mikan_input.h`.
//!
//! The values are USB HID keyboard usage IDs (usage page 1), a stable
//! documented numbering that maps one-to-one onto the engine's key names.
//! Code `0` ([`KEY_UNKNOWN`]) is reserved: raw key state under it is tracked
//! by the backend, but it can never be bound to an action.

/// The reserved unknown-key code; tracked but never bindable.
pub const KEY_UNKNOWN: u32 = 0;
/// `MIKAN_INPUT_KEY_A`.
pub const KEY_A: u32 = 0x04;
/// `MIKAN_INPUT_KEY_B`.
pub const KEY_B: u32 = 0x05;
/// `MIKAN_INPUT_KEY_C`.
pub const KEY_C: u32 = 0x06;
/// `MIKAN_INPUT_KEY_D`.
pub const KEY_D: u32 = 0x07;
/// `MIKAN_INPUT_KEY_E`.
pub const KEY_E: u32 = 0x08;
/// `MIKAN_INPUT_KEY_F`.
pub const KEY_F: u32 = 0x09;
/// `MIKAN_INPUT_KEY_G`.
pub const KEY_G: u32 = 0x0A;
/// `MIKAN_INPUT_KEY_H`.
pub const KEY_H: u32 = 0x0B;
/// `MIKAN_INPUT_KEY_I`.
pub const KEY_I: u32 = 0x0C;
/// `MIKAN_INPUT_KEY_J`.
pub const KEY_J: u32 = 0x0D;
/// `MIKAN_INPUT_KEY_K`.
pub const KEY_K: u32 = 0x0E;
/// `MIKAN_INPUT_KEY_L`.
pub const KEY_L: u32 = 0x0F;
/// `MIKAN_INPUT_KEY_M`.
pub const KEY_M: u32 = 0x10;
/// `MIKAN_INPUT_KEY_N`.
pub const KEY_N: u32 = 0x11;
/// `MIKAN_INPUT_KEY_O`.
pub const KEY_O: u32 = 0x12;
/// `MIKAN_INPUT_KEY_P`.
pub const KEY_P: u32 = 0x13;
/// `MIKAN_INPUT_KEY_Q`.
pub const KEY_Q: u32 = 0x14;
/// `MIKAN_INPUT_KEY_R`.
pub const KEY_R: u32 = 0x15;
/// `MIKAN_INPUT_KEY_S`.
pub const KEY_S: u32 = 0x16;
/// `MIKAN_INPUT_KEY_T`.
pub const KEY_T: u32 = 0x17;
/// `MIKAN_INPUT_KEY_U`.
pub const KEY_U: u32 = 0x18;
/// `MIKAN_INPUT_KEY_V`.
pub const KEY_V: u32 = 0x19;
/// `MIKAN_INPUT_KEY_W`.
pub const KEY_W: u32 = 0x1A;
/// `MIKAN_INPUT_KEY_X`.
pub const KEY_X: u32 = 0x1B;
/// `MIKAN_INPUT_KEY_Y`.
pub const KEY_Y: u32 = 0x1C;
/// `MIKAN_INPUT_KEY_Z`.
pub const KEY_Z: u32 = 0x1D;
/// `MIKAN_INPUT_KEY_DIGIT1`.
pub const KEY_DIGIT1: u32 = 0x1E;
/// `MIKAN_INPUT_KEY_DIGIT2`.
pub const KEY_DIGIT2: u32 = 0x1F;
/// `MIKAN_INPUT_KEY_DIGIT3`.
pub const KEY_DIGIT3: u32 = 0x20;
/// `MIKAN_INPUT_KEY_DIGIT4`.
pub const KEY_DIGIT4: u32 = 0x21;
/// `MIKAN_INPUT_KEY_DIGIT5`.
pub const KEY_DIGIT5: u32 = 0x22;
/// `MIKAN_INPUT_KEY_DIGIT6`.
pub const KEY_DIGIT6: u32 = 0x23;
/// `MIKAN_INPUT_KEY_DIGIT7`.
pub const KEY_DIGIT7: u32 = 0x24;
/// `MIKAN_INPUT_KEY_DIGIT8`.
pub const KEY_DIGIT8: u32 = 0x25;
/// `MIKAN_INPUT_KEY_DIGIT9`.
pub const KEY_DIGIT9: u32 = 0x26;
/// `MIKAN_INPUT_KEY_DIGIT0`.
pub const KEY_DIGIT0: u32 = 0x27;
/// `MIKAN_INPUT_KEY_ENTER`.
pub const KEY_ENTER: u32 = 0x28;
/// `MIKAN_INPUT_KEY_ESCAPE`.
pub const KEY_ESCAPE: u32 = 0x29;
/// `MIKAN_INPUT_KEY_BACKSPACE`.
pub const KEY_BACKSPACE: u32 = 0x2A;
/// `MIKAN_INPUT_KEY_TAB`.
pub const KEY_TAB: u32 = 0x2B;
/// `MIKAN_INPUT_KEY_SPACE`.
pub const KEY_SPACE: u32 = 0x2C;
/// `MIKAN_INPUT_KEY_MINUS`.
pub const KEY_MINUS: u32 = 0x2D;
/// `MIKAN_INPUT_KEY_EQUAL`.
pub const KEY_EQUAL: u32 = 0x2E;
/// `MIKAN_INPUT_KEY_BRACKETLEFT`.
pub const KEY_BRACKETLEFT: u32 = 0x2F;
/// `MIKAN_INPUT_KEY_BRACKETRIGHT`.
pub const KEY_BRACKETRIGHT: u32 = 0x30;
/// `MIKAN_INPUT_KEY_BACKSLASH`.
pub const KEY_BACKSLASH: u32 = 0x31;
/// `MIKAN_INPUT_KEY_SEMICOLON`.
pub const KEY_SEMICOLON: u32 = 0x33;
/// `MIKAN_INPUT_KEY_QUOTE`.
pub const KEY_QUOTE: u32 = 0x34;
/// `MIKAN_INPUT_KEY_BACKQUOTE`.
pub const KEY_BACKQUOTE: u32 = 0x35;
/// `MIKAN_INPUT_KEY_COMMA`.
pub const KEY_COMMA: u32 = 0x36;
/// `MIKAN_INPUT_KEY_PERIOD`.
pub const KEY_PERIOD: u32 = 0x37;
/// `MIKAN_INPUT_KEY_SLASH`.
pub const KEY_SLASH: u32 = 0x38;
/// `MIKAN_INPUT_KEY_CAPSLOCK`.
pub const KEY_CAPSLOCK: u32 = 0x39;
/// `MIKAN_INPUT_KEY_F1`.
pub const KEY_F1: u32 = 0x3A;
/// `MIKAN_INPUT_KEY_F2`.
pub const KEY_F2: u32 = 0x3B;
/// `MIKAN_INPUT_KEY_F3`.
pub const KEY_F3: u32 = 0x3C;
/// `MIKAN_INPUT_KEY_F4`.
pub const KEY_F4: u32 = 0x3D;
/// `MIKAN_INPUT_KEY_F5`.
pub const KEY_F5: u32 = 0x3E;
/// `MIKAN_INPUT_KEY_F6`.
pub const KEY_F6: u32 = 0x3F;
/// `MIKAN_INPUT_KEY_F7`.
pub const KEY_F7: u32 = 0x40;
/// `MIKAN_INPUT_KEY_F8`.
pub const KEY_F8: u32 = 0x41;
/// `MIKAN_INPUT_KEY_F9`.
pub const KEY_F9: u32 = 0x42;
/// `MIKAN_INPUT_KEY_F10`.
pub const KEY_F10: u32 = 0x43;
/// `MIKAN_INPUT_KEY_F11`.
pub const KEY_F11: u32 = 0x44;
/// `MIKAN_INPUT_KEY_F12`.
pub const KEY_F12: u32 = 0x45;
/// `MIKAN_INPUT_KEY_INSERT`.
pub const KEY_INSERT: u32 = 0x49;
/// `MIKAN_INPUT_KEY_HOME`.
pub const KEY_HOME: u32 = 0x4A;
/// `MIKAN_INPUT_KEY_PAGEUP`.
pub const KEY_PAGEUP: u32 = 0x4B;
/// `MIKAN_INPUT_KEY_DELETE`.
pub const KEY_DELETE: u32 = 0x4C;
/// `MIKAN_INPUT_KEY_END`.
pub const KEY_END: u32 = 0x4D;
/// `MIKAN_INPUT_KEY_PAGEDOWN`.
pub const KEY_PAGEDOWN: u32 = 0x4E;
/// `MIKAN_INPUT_KEY_ARROWRIGHT`.
pub const KEY_ARROWRIGHT: u32 = 0x4F;
/// `MIKAN_INPUT_KEY_ARROWLEFT`.
pub const KEY_ARROWLEFT: u32 = 0x50;
/// `MIKAN_INPUT_KEY_ARROWDOWN`.
pub const KEY_ARROWDOWN: u32 = 0x51;
/// `MIKAN_INPUT_KEY_ARROWUP`.
pub const KEY_ARROWUP: u32 = 0x52;
/// `MIKAN_INPUT_KEY_CONTROLLEFT`.
pub const KEY_CONTROLLEFT: u32 = 0xE0;
/// `MIKAN_INPUT_KEY_SHIFTLEFT`.
pub const KEY_SHIFTLEFT: u32 = 0xE1;
/// `MIKAN_INPUT_KEY_ALTLEFT`.
pub const KEY_ALTLEFT: u32 = 0xE2;
/// `MIKAN_INPUT_KEY_METALEFT`.
pub const KEY_METALEFT: u32 = 0xE3;
/// `MIKAN_INPUT_KEY_CONTROLRIGHT`.
pub const KEY_CONTROLRIGHT: u32 = 0xE4;
/// `MIKAN_INPUT_KEY_SHIFTRIGHT`.
pub const KEY_SHIFTRIGHT: u32 = 0xE5;
/// `MIKAN_INPUT_KEY_ALTRIGHT`.
pub const KEY_ALTRIGHT: u32 = 0xE6;
/// `MIKAN_INPUT_KEY_METARIGHT`.
pub const KEY_METARIGHT: u32 = 0xE7;
