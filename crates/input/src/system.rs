//! The safe input system: action contexts, mapping, and the per-step pipeline.
//!
//! [`InputSystem`] owns the native backend and runtime and translates the
//! platform's [`DeviceEvent`] stream into them, so every `unsafe` lives in
//! `mikan-input-sys` and this module is unsafe-free. Its API names what the
//! game layer cares about: [`ActionId`], [`ContextId`], [`Binding`], queries,
//! and the end-of-step boundary.

use std::collections::HashMap;
use std::fmt;

use mikan_input_sys as inf;
use mikan_input_sys::keys;
use mikan_platform::DeviceEvent;
use thiserror::Error;
use winit::event::MouseButton;
use winit::keyboard::PhysicalKey;

use crate::map::{key_code, mouse_button};

/// Identifies an action. Its meaning stays with the application; the native
/// module only sees the number.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionId(u32);

impl ActionId {
    /// Creates an id.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The numeric action id handed to the native module.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "action {}", self.0)
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "action {}", self.0)
    }
}

impl From<u32> for ActionId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// Identifies an input context. Ids are assigned in creation order starting
/// at 1 and stay stable while the system lives (destroyed ids are never
/// reused).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextId(u32);

impl ContextId {
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The numeric context id handed to the native module.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ContextId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "context {}", self.0)
    }
}

impl fmt::Display for ContextId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "context {}", self.0)
    }
}

/// One bindable input: a named key, a named mouse button, or a wheel axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// A physical key.
    Key(PhysicalKey),
    /// A mouse button.
    Button(MouseButton),
    /// A wheel axis; the direction of the scroll decides the action's value.
    Wheel(WheelAxis),
}

impl Binding {
    /// `(device, code)` in native-module terms.
    fn parts(self) -> (u32, u32) {
        match self {
            Self::Key(key) => (inf::EVENT_KEY_PRESS, key_code(key)),
            Self::Button(button) => (inf::EVENT_MOUSE_PRESS, mouse_button(button)),
            Self::Wheel(axis) => (inf::EVENT_WHEEL, u32::from(axis)),
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => write!(f, "key {key:?}"),
            Self::Button(button) => write!(f, "mouse {button:?}"),
            Self::Wheel(axis) => write!(f, "wheel {axis:?}"),
        }
    }
}

/// A wheel axis, matching the native module's two axis codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WheelAxis {
    /// Horizontal scroll.
    Horizontal,
    /// Vertical scroll.
    Vertical,
}

impl From<WheelAxis> for u32 {
    fn from(axis: WheelAxis) -> Self {
        match axis {
            WheelAxis::Horizontal => inf::WHEEL_HORIZONTAL,
            WheelAxis::Vertical => inf::WHEEL_VERTICAL,
        }
    }
}

/// One action's state after the events processed so far in this step.
pub use inf::ActionState;

/// Why an [`InputSystem`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum InputError {
    /// The key has no HID id (see `key_code`) and cannot be bound.
    #[error("`{0}` has no stable code and cannot be bound")]
    UnknownKey(String),
    /// The input is already bound to an action in that context. Nothing was
    /// changed; unbind first to move a binding.
    #[error("{input} is already bound in {context}")]
    DuplicateBinding {
        /// The affected context.
        context: ContextId,
        /// The conflicting input.
        input: String,
    },
    /// The unbind call named an input that context does not bind.
    #[error("{input} is not bound in {context}")]
    BindingNotFound {
        /// The affected context.
        context: ContextId,
        /// The missing input.
        input: String,
    },
    /// An active context cannot be removed; deactivate it first.
    #[error("{0} is still active and must be deactivated before removal")]
    ContextActive(ContextId),
    /// The id refers to a removed (or never-created) context.
    #[error("{0} does not exist")]
    UnknownContext(ContextId),
    /// The native event buffer could not hold every batch event; the excess
    /// was dropped and counted, and the kept events were still processed.
    #[error("the input event buffer overflowed; {dropped} event(s) were dropped and counted")]
    Overflow {
        /// The backend's cumulative dropped-event counter.
        dropped: usize,
    },
    /// A native-module failure outside the states the API can produce; treat
    /// it as a bug and report it.
    #[error("native input module failure: {0:?}")]
    Native(inf::RuntimeError),
}

/// The safe face of the native input module: device events in, actions out.
///
/// One system per game. It is neither `Send` nor `Sync`, which mirrors the
/// native module's single-thread contract and the engine's event ownership:
/// everything happens on the main thread, where the runner handles events.
///
/// The intended rhythm inside a fixed update is:
///
/// 1. drain the platform's device-event log and call
///    [`InputSystem::process`] — the first step of a frame sees everything;
///    later steps of the same frame see an empty rest, which is fine;
/// 2. query: [`InputSystem::held`], [`InputSystem::just_pressed`],
///    [`InputSystem::just_released`], [`InputSystem::value`];
/// 3. call [`InputSystem::end_step`] to clear the step-scoped edges.
pub struct InputSystem {
    backend: inf::Backend,
    runtime: inf::Runtime,
    /// Names of live contexts, for error messages and `Debug`. Kept in
    /// lockstep with the native runtime's context set.
    contexts: HashMap<u32, String>,
    /// Scratch space reused between `process` calls.
    scratch: Vec<inf::InputEvent>,
}

impl fmt::Debug for InputSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputSystem")
            .field("contexts", &self.contexts)
            .field("dropped", &self.dropped())
            .finish()
    }
}

impl InputSystem {
    /// Creates a system with the native module's default event-buffer
    /// capacity (4096 events), which is far beyond what one frame of human
    /// input produces.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backend: inf::Backend::new(0),
            runtime: inf::Runtime::new(),
            contexts: HashMap::new(),
            scratch: Vec::new(),
        }
    }

    /// Creates a context with native priority `priority`.
    ///
    /// Higher priority wins contested input in the same step; equal
    /// priorities fall back to later-created contexts.
    #[must_use]
    pub fn create_context(&mut self, name: &str, priority: u32) -> ContextId {
        // SAFETY contract of `add_context`: it fails only on allocation
        // failure or bad arguments, and this call supplies neither (the
        // wrapper panics on allocation failure; the id comes back unset).
        let id = self
            .runtime
            .add_context(priority)
            .expect("a new runtime always accepts another context");
        self.contexts.insert(id, name.to_string());
        ContextId::new(id)
    }

    /// Removes an inactive context and every binding in it.
    ///
    /// # Errors
    ///
    /// [`InputError::ContextActive`] if the context is still active;
    /// [`InputError::UnknownContext`] if the id is stale.
    pub fn destroy_context(&mut self, context: ContextId) -> Result<(), InputError> {
        match self.runtime.remove_context(context.to_raw()) {
            Ok(()) => {
                self.contexts.remove(&context.to_raw());
                Ok(())
            }
            Err(inf::RuntimeError::BadArgument) => Err(InputError::ContextActive(context)),
            Err(inf::RuntimeError::UnknownContext) => Err(InputError::UnknownContext(context)),
            Err(other) => Err(InputError::Native(other)),
        }
    }

    /// Activates or deactivates a context. Deactivating releases every
    /// binding the context was holding (with the usual just-released edges);
    /// nothing auto-presses on activation.
    ///
    /// # Errors
    ///
    /// [`InputError::UnknownContext`] if the id is stale.
    pub fn set_active(&mut self, context: ContextId, active: bool) -> Result<(), InputError> {
        match self.runtime.set_context_active(context.to_raw(), active) {
            Ok(()) => Ok(()),
            Err(inf::RuntimeError::UnknownContext) => Err(InputError::UnknownContext(context)),
            Err(other) => Err(InputError::Native(other)),
        }
    }

    /// Whether the context is live.
    #[must_use]
    pub fn context_exists(&self, context: ContextId) -> bool {
        self.contexts.contains_key(&context.to_raw())
    }

    /// The number of bindings a context holds; `0` when it does not exist.
    #[must_use]
    pub fn binding_count(&self, context: ContextId) -> usize {
        self.runtime
            .binding_count(context.to_raw())
            .unwrap_or_default()
    }

    /// Binds one device input to `action` inside `context`.
    ///
    /// # Errors
    ///
    /// [`InputError::UnknownKey`] for a key with no stable code;
    /// [`InputError::DuplicateBinding`] if the input is already bound to a
    /// different action there; [`InputError::UnknownContext`] if the id is
    /// stale.
    pub fn bind(
        &mut self,
        context: ContextId,
        binding: Binding,
        action: impl Into<ActionId>,
    ) -> Result<(), InputError> {
        let action = action.into();
        let (device, code) = binding.parts();
        if device == inf::EVENT_KEY_PRESS && code == keys::KEY_UNKNOWN {
            // Key code 0 is the reserved unknown-key code. Mouse buttons and
            // wheel axes are never translated to it.
            return Err(InputError::UnknownKey(binding.to_string()));
        }
        match self
            .runtime
            .bind(context.to_raw(), device, code, action.to_raw())
        {
            Ok(()) => Ok(()),
            Err(inf::RuntimeError::Conflict) => Err(InputError::DuplicateBinding {
                context,
                input: binding.to_string(),
            }),
            Err(inf::RuntimeError::UnknownContext) => Err(InputError::UnknownContext(context)),
            Err(other) => Err(InputError::Native(other)),
        }
    }

    /// Removes one binding from a context.
    ///
    /// # Errors
    ///
    /// [`InputError::BindingNotFound`] if the input is not bound there;
    /// [`InputError::UnknownContext`] if the id is stale.
    pub fn unbind(&mut self, context: ContextId, binding: Binding) -> Result<(), InputError> {
        let (device, code) = binding.parts();
        match self.runtime.unbind(context.to_raw(), device, code) {
            Ok(()) => Ok(()),
            Err(inf::RuntimeError::NotFound) => Err(InputError::BindingNotFound {
                context,
                input: binding.to_string(),
            }),
            Err(inf::RuntimeError::UnknownContext) => Err(InputError::UnknownContext(context)),
            Err(other) => Err(InputError::Native(other)),
        }
    }

    /// Processes one batch of raw device events: feeds them to the native
    /// backend, then drains the normalized result straight into the runtime.
    ///
    /// Call this once per fixed step, before queries. The runner records
    /// every device event in the platform's log from
    /// [`InputState::take_events`], which the application drains as its very
    /// first input-related action each step.
    ///
    /// # Errors
    ///
    /// [`InputError::Overflow`] if the native buffer could not hold a batch;
    /// the dropped count is cumulative since the system was created and also
    /// readable through [`InputSystem::dropped`]. State stays consistent
    /// either way.
    ///
    /// [`InputState::take_events`]: mikan_platform::InputState::take_events
    pub fn process(&mut self, events: &[DeviceEvent]) -> Result<(), InputError> {
        for event in events {
            let result = match *event {
                DeviceEvent::Key {
                    key,
                    pressed,
                    repeat,
                } => self.backend.push_key(key_code(key), pressed, repeat),
                DeviceEvent::MouseButton { button, pressed } => self
                    .backend
                    .push_mouse_button(mouse_button(button), pressed),
                DeviceEvent::CursorMoved { position } => {
                    self.backend.push_mouse_move(position.x, position.y)
                }
                DeviceEvent::MouseWheel { delta: (dx, dy) } => self.backend.push_wheel(dx, dy),
                DeviceEvent::FocusChanged { focused } => self.backend.push_focus(focused),
            };
            if result.is_err() {
                return Err(self.overflow_error());
            }
        }
        if self.backend.drain(&mut self.scratch).is_err() {
            return Err(self.overflow_error());
        }
        // The conversion above can only produce well-formed events, so the
        // runtime's `process` has no reachable failure mode here.
        self.runtime
            .process(&self.scratch)
            .expect("a converted event batch is always well-formed");
        Ok(())
    }

    /// Ends the current step: clears `just_pressed`, `just_released`, and
    /// `repeating`, and resets the wheel contribution to every action's
    /// `value`. Held state and raw device state persist.
    pub fn end_step(&mut self) {
        self.runtime
            .end_step()
            .expect("ending a step has no reachable failure");
    }

    /// Whether `action` is currently held (at least one bound input is down).
    #[must_use]
    pub fn held(&self, action: impl Into<ActionId>) -> bool {
        self.state(action).is_some_and(|state| state.held)
    }

    /// Whether `action` went from released to held during this step.
    #[must_use]
    pub fn just_pressed(&self, action: impl Into<ActionId>) -> bool {
        self.state(action).is_some_and(|state| state.just_pressed)
    }

    /// Whether `action` went from held to released during this step.
    #[must_use]
    pub fn just_released(&self, action: impl Into<ActionId>) -> bool {
        self.state(action).is_some_and(|state| state.just_released)
    }

    /// Whether a bound key repeated while held during this step.
    #[must_use]
    pub fn repeating(&self, action: impl Into<ActionId>) -> bool {
        self.state(action).is_some_and(|state| state.repeating)
    }

    /// The action's value: `1.0` while held plus the step's wheel deltas
    /// bound to it, `0.0` when idle. `end_step` recomputes it exactly from
    /// held state, so drift cannot accumulate.
    #[must_use]
    pub fn value(&self, action: impl Into<ActionId>) -> f32 {
        self.state(action).map_or(0.0, |state| state.value)
    }

    /// Full state of one action.
    #[must_use]
    pub fn action_state(&self, action: impl Into<ActionId>) -> Option<ActionState> {
        self.runtime.action_state(action.into().to_raw()).ok()
    }

    /// How many events the native backend dropped since the system was
    /// created.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.backend.dropped()
    }

    fn state(&self, action: impl Into<ActionId>) -> Option<inf::ActionState> {
        self.runtime.action_state(action.into().to_raw()).ok()
    }

    fn overflow_error(&self) -> InputError {
        InputError::Overflow {
            dropped: self.backend.dropped(),
        }
    }
}

impl Default for InputSystem {
    fn default() -> Self {
        Self::new()
    }
}
