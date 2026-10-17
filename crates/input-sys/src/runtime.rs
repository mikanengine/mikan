//! The safe RAII wrapper around the native action runtime.
//!
//! [`Runtime`] owns one opaque `mikan_input_runtime` handle: created exactly
//! once by [`Runtime::new`], destroyed exactly once by its `Drop`, and never
//! exposed as a pointer. Its methods translate the C++ module's status codes
//! into [`RuntimeError`] and uphold every precondition the header states —
//! no null handles, exclusive access through `&mut self`.
//!
//! The type is neither `Send` nor `Sync`, matching the header's single-thread
//! contract; the raw pointer inside enforces both at compile time.

use core::ptr::NonNull;

use crate::{
    CONFLICT, InputActionState, InputEvent, InputRuntime, NOT_FOUND, OK, UNKNOWN_CONTEXT,
    mikan_input_runtime_action_state, mikan_input_runtime_add_context, mikan_input_runtime_bind,
    mikan_input_runtime_binding_count, mikan_input_runtime_create, mikan_input_runtime_destroy,
    mikan_input_runtime_end_step, mikan_input_runtime_process, mikan_input_runtime_remove_context,
    mikan_input_runtime_set_context_active, mikan_input_runtime_unbind,
};

/// A runtime operation failed; see the individual methods for which codes
/// they can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// The argument was rejected: an invalid device kind, a reserved code 0,
    /// or removal of an active context.
    BadArgument,
    /// The unbind call named an input that is not bound in that context.
    NotFound,
    /// The input is already bound in that context; nothing was changed.
    Conflict,
    /// The context id is not in the runtime.
    UnknownContext,
}

fn to_runtime_error(status: i32) -> RuntimeError {
    if status == NOT_FOUND {
        RuntimeError::NotFound
    } else if status == CONFLICT {
        RuntimeError::Conflict
    } else if status == UNKNOWN_CONTEXT {
        RuntimeError::UnknownContext
    } else {
        RuntimeError::BadArgument
    }
}

/// One action's state after the events processed so far in this step.
///
/// `value` is `1.0` while held plus the wheel deltas bound to the action
/// this step, and `0.0` when idle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionState {
    /// At least one bound input is currently down.
    pub held: bool,
    /// The action went from released to held in this step.
    pub just_pressed: bool,
    /// It went from held to released in this step.
    pub just_released: bool,
    /// A bound key repeated while held in this step.
    pub repeating: bool,
    /// The analog value: held plus the step's wheel contribution.
    pub value: f32,
}

/// An owning handle to one native action runtime.
pub struct Runtime {
    handle: NonNull<InputRuntime>,
}

impl Runtime {
    /// Creates an empty runtime with no contexts and no bindings.
    ///
    /// # Panics
    ///
    /// If the native allocation fails — the same failure mode as any other
    /// allocation, reported where it happens rather than threaded through
    /// every caller as a `Result`.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: `create` takes no preconditions and returns either null or
        // a fresh, exclusively-owned handle, which `NonNull::new` filters and
        // `Drop` frees exactly once.
        let handle = unsafe { mikan_input_runtime_create() };
        let Some(handle) = NonNull::new(handle) else {
            panic!("mikan-input-sys: the native input runtime allocation failed");
        };
        Self { handle }
    }

    /// Adds a context and returns its id. Context ids are assigned in
    /// creation order starting at 1.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn add_context(&mut self, priority: u32) -> Result<u32, RuntimeError> {
        let mut id = 0_u32;
        // SAFETY: `self.handle` is live and `&mut self` proves exclusive
        // access, which is the module's threading contract; `id` is a live
        // local the module writes.
        let status =
            unsafe { mikan_input_runtime_add_context(self.handle.as_ptr(), priority, &mut id) };
        if status == OK {
            Ok(id)
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Activates or deactivates a context. Deactivating releases everything
    /// the context held down (with the usual just-released edges); nothing
    /// auto-presses on activation.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::UnknownContext`] if `context` does not exist.
    pub fn set_context_active(&mut self, context: u32, active: bool) -> Result<(), RuntimeError> {
        // SAFETY: as `add_context`.
        let status = unsafe {
            mikan_input_runtime_set_context_active(self.handle.as_ptr(), context, active as i32)
        };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Removes an inactive context. Its bindings go with it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] if the context is active;
    /// [`RuntimeError::UnknownContext`] if it does not exist.
    pub fn remove_context(&mut self, context: u32) -> Result<(), RuntimeError> {
        // SAFETY: as `add_context`.
        let status = unsafe { mikan_input_runtime_remove_context(self.handle.as_ptr(), context) };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Binds one key input (`device` [`crate::EVENT_KEY_PRESS`]) to `action`
    /// inside `context`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Conflict`] if the input is already bound there;
    /// [`RuntimeError::BadArgument`] for key code 0;
    /// [`RuntimeError::UnknownContext`] if the context does not exist.
    pub fn bind_key(&mut self, context: u32, code: u32, action: u32) -> Result<(), RuntimeError> {
        self.bind(context, crate::EVENT_KEY_PRESS, code, action)
    }

    /// Binds one mouse-button input to `action` inside `context`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Conflict`] if the input is already bound there;
    /// [`RuntimeError::BadArgument`] for button code 0;
    /// [`RuntimeError::UnknownContext`] if the context does not exist.
    pub fn bind_mouse_button(
        &mut self,
        context: u32,
        code: u32,
        action: u32,
    ) -> Result<(), RuntimeError> {
        self.bind(context, crate::EVENT_MOUSE_PRESS, code, action)
    }

    /// Binds one wheel axis to `action` inside `context`. The axis is
    /// [`crate::WHEEL_HORIZONTAL`] or [`crate::WHEEL_VERTICAL`].
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Conflict`] if the axis is already bound there;
    /// [`RuntimeError::BadArgument`] for another axis code;
    /// [`RuntimeError::UnknownContext`] if the context does not exist.
    pub fn bind_wheel(&mut self, context: u32, axis: u32, action: u32) -> Result<(), RuntimeError> {
        self.bind(context, crate::EVENT_WHEEL, axis, action)
    }

    /// The shared bind call; see [`Runtime::bind_key`] and friends.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Conflict`], [`RuntimeError::BadArgument`],
    /// [`RuntimeError::UnknownContext`] as the module reports them.
    pub fn bind(
        &mut self,
        context: u32,
        device: u32,
        code: u32,
        action: u32,
    ) -> Result<(), RuntimeError> {
        // SAFETY: as `add_context`.
        let status = unsafe {
            mikan_input_runtime_bind(self.handle.as_ptr(), context, device, code, action)
        };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Removes the binding of one device input from a context.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] if the input is not bound there;
    /// [`RuntimeError::UnknownContext`] if the context does not exist.
    pub fn unbind(&mut self, context: u32, device: u32, code: u32) -> Result<(), RuntimeError> {
        // SAFETY: as `add_context`.
        let status =
            unsafe { mikan_input_runtime_unbind(self.handle.as_ptr(), context, device, code) };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// The number of bindings in a context.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::UnknownContext`] if the context does not exist.
    pub fn binding_count(&self, context: u32) -> Result<usize, RuntimeError> {
        let mut count = 0_usize;
        // SAFETY: `self.handle` is live; `count` is a live local the module
        // writes.
        let status =
            unsafe { mikan_input_runtime_binding_count(self.handle.as_ptr(), context, &mut count) };
        if status == OK {
            Ok(count)
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Processes one batch of normalized events, in stream order.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn process(&mut self, events: &[InputEvent]) -> Result<(), RuntimeError> {
        // SAFETY: `self.handle` is live and borrowed exclusively; `events`
        // is a live slice valid for `events.len()` reads (an empty slice's
        // pointer is still valid for zero reads).
        let status = unsafe {
            mikan_input_runtime_process(self.handle.as_ptr(), events.as_ptr(), events.len())
        };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Ends the current step: clears the just-pressed, just-released, and
    /// repeating flags and the wheel contribution to `value`. Held state
    /// persists until the matching release.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn end_step(&mut self) -> Result<(), RuntimeError> {
        // SAFETY: as `add_context`.
        let status = unsafe { mikan_input_runtime_end_step(self.handle.as_ptr()) };
        if status == OK {
            Ok(())
        } else {
            Err(to_runtime_error(status))
        }
    }

    /// Reads one action's state. An action with no bindings reads as idle.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn action_state(&self, action: u32) -> Result<ActionState, RuntimeError> {
        let mut state = InputActionState::default();
        // SAFETY: `self.handle` is live; `state` is a live local the module
        // writes.
        let status =
            unsafe { mikan_input_runtime_action_state(self.handle.as_ptr(), action, &mut state) };
        if status != OK {
            return Err(to_runtime_error(status));
        }
        Ok(ActionState {
            held: state.held != 0,
            just_pressed: state.just_pressed != 0,
            just_released: state.just_released != 0,
            repeating: state.repeating != 0,
            value: state.value,
        })
    }

    /// Convenience: whether `action` is held.
    ///
    /// # Errors
    ///
    /// As [`Runtime::action_state`].
    pub fn is_action_held(&self, action: u32) -> Result<bool, RuntimeError> {
        Ok(self.action_state(action)?.held)
    }

    /// Convenience: whether `action` went down in this step.
    ///
    /// # Errors
    ///
    /// As [`Runtime::action_state`].
    pub fn is_action_just_pressed(&self, action: u32) -> Result<bool, RuntimeError> {
        Ok(self.action_state(action)?.just_pressed)
    }

    /// Convenience: whether `action` came up in this step.
    ///
    /// # Errors
    ///
    /// As [`Runtime::action_state`].
    pub fn is_action_just_released(&self, action: u32) -> Result<bool, RuntimeError> {
        Ok(self.action_state(action)?.just_released)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from `create` and is dropped exactly
        // once, here; no method can use it afterwards.
        unsafe { mikan_input_runtime_destroy(self.handle.as_ptr()) };
    }
}

impl core::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Runtime").finish_non_exhaustive()
    }
}
