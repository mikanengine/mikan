//! The input system: raw device events in, named actions out.
//!
//! This crate is the safe face of Phase 8. It owns three decisions the rest
//! of the engine leans on:
//!
//! 1. **Device events stay raw.** The platform runner records a
//!    [`DeviceEvent`] stream ([`InputState::take_events`]); an application
//!    drains it once per fixed step into [`InputSystem::process`].
//! 2. **Actions, not keys, are the gameplay API.** Reads go through
//!    [`InputSystem::held`], [`just_pressed`], [`just_released`] and
//!    [`value`], so remapping a key is a one-line `bind` change.
//! 3. **Contexts arbitrate.** Each [`ContextId`] is a set of bindings with a
//!    priority; only the highest-priority active context consumes contested
//!    input, which is how menus and gameplay can both read the wheel without
//!    stepping on each other.
//!
//! The native C backend and C++ action runtime live in `mikan-input-sys`;
//! this crate contains no `unsafe`.
//!
//! [`InputState::take_events`]: mikan_platform::InputState::take_events
//! [`just_pressed`]: InputSystem::just_pressed
//! [`just_released`]: InputSystem::just_released
//! [`value`]: InputSystem::value
//!
//! # Example
//!
//! ```
//! use mikan_input::{ActionId, Binding, InputSystem, WheelAxis};
//! use mikan_platform::DeviceEvent;
//! use winit::keyboard::{KeyCode, PhysicalKey};
//!
//! let mut input = InputSystem::new();
//! let gameplay: mikan_input::ContextId = input.create_context("gameplay", 0);
//! let move_right = ActionId::new(1);
//! input
//!     .bind(gameplay, Binding::Key(PhysicalKey::Code(KeyCode::KeyD)), move_right)
//!     .expect("KeyD has a stable code");
//! input
//!     .bind(gameplay, Binding::Wheel(WheelAxis::Vertical), move_right)
//!     .expect("the vertical wheel axis binds once");
//!
//! input
//!     .process(&[
//!         DeviceEvent::Key { key: PhysicalKey::Code(KeyCode::KeyD), pressed: true, repeat: false },
//!     ])
//!     .expect("a clerk batch never overflows");
//! assert!(input.just_pressed(move_right));
//! input.end_step();
//! assert!(input.held(move_right), "held state survives the step boundary");
//! assert!(!input.just_pressed(move_right));
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod map;
mod system;

pub use system::{ActionId, ActionState, Binding, ContextId, InputError, InputSystem, WheelAxis};
