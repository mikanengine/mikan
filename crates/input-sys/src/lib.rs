//! Raw interface to the native input module (`mikan_input`).
//!
//! This crate is the ABI mirror of `src/mikan_input.h`: the `#[repr(C)]`
//! event struct, the status codes, the key codes, and the `extern "C"`
//! declarations for the two native layers — the C device backend
//! (`input_backend.c`) and the C++ action runtime (`input_runtime.cpp`).
//! `mikan-input` is the crate applications should depend on; it wraps both
//! handles in safe types and owns the frame and lifecycle integration.
//!
//! # Why `unsafe` is allowed in this crate
//!
//! The workspace forbids `unsafe_code` crate-wide. This is the crate that
//! bridges into the native input module, and the rules that make the calls
//! sound — opaque handle validity, buffer lengths, exclusive access to a
//! backend or runtime — are a C ABI contract the Rust type system cannot
//! check. The override is recorded here, per `docs/ffi.md`; every occurrence
//! carries a `// SAFETY:` comment; the safe wrapper (`mikan-input`) and every
//! consumer contain none.
//!
//! # Layout and ownership
//!
//! [`InputEvent`] is plain data read and written by value across the
//! boundary; neither side retains a pointer to it. A [`Backend`] handle or a
//! [`Runtime`] handle is owned by the caller: the matching `create` allocates
//! it, the matching `destroy` frees it, and each must be destroyed exactly
//! once per successful create — which the RAII wrappers in [`backend`] and
//! [`runtime`] enforce. Neither is thread-safe: every call that takes a
//! non-const handle requires exclusive access, which the wrappers encode as
//! `&mut self`.
//!
//! # The Rust reference
//!
//! [`reference`] is an independent Rust re-implementation of the backend's
//! normalization rules. It is the test oracle: the C backend's event streams
//! are cross-checked against it on fixed and randomized sequences, bit for
//! bit. That is a requirement of the module, not duplication for its own
//! sake; the module says so.

use core::ffi::c_int;

pub mod backend;
pub mod keys;
pub mod reference;
pub mod runtime;

pub use backend::{Backend, BackendError};
pub use runtime::{ActionState, Runtime, RuntimeError};

/// `MIKAN_INPUT_EVENT_KEY_PRESS`: a key went down.
pub const EVENT_KEY_PRESS: u32 = 1;
/// `MIKAN_INPUT_EVENT_KEY_RELEASE`: a key came up.
pub const EVENT_KEY_RELEASE: u32 = 2;
/// `MIKAN_INPUT_EVENT_KEY_REPEAT`: an operating-system repeat of a held key.
pub const EVENT_KEY_REPEAT: u32 = 3;
/// `MIKAN_INPUT_EVENT_MOUSE_PRESS`: a mouse button went down.
pub const EVENT_MOUSE_PRESS: u32 = 4;
/// `MIKAN_INPUT_EVENT_MOUSE_RELEASE`: a mouse button came up.
pub const EVENT_MOUSE_RELEASE: u32 = 5;
/// `MIKAN_INPUT_EVENT_MOUSE_MOVE`: the cursor is at an absolute position.
pub const EVENT_MOUSE_MOVE: u32 = 6;
/// `MIKAN_INPUT_EVENT_WHEEL`: a scroll; `x` and `y` are the deltas.
pub const EVENT_WHEEL: u32 = 7;
/// `MIKAN_INPUT_EVENT_FOCUS_LOST`: the window lost focus.
pub const EVENT_FOCUS_LOST: u32 = 8;
/// `MIKAN_INPUT_EVENT_FOCUS_GAINED`: the window gained focus.
pub const EVENT_FOCUS_GAINED: u32 = 9;

/// `MIKAN_INPUT_MOUSE_NONE`.
pub const MOUSE_NONE: u32 = 0;
/// `MIKAN_INPUT_MOUSE_LEFT`.
pub const MOUSE_LEFT: u32 = 1;
/// `MIKAN_INPUT_MOUSE_RIGHT`.
pub const MOUSE_RIGHT: u32 = 2;
/// `MIKAN_INPUT_MOUSE_MIDDLE`.
pub const MOUSE_MIDDLE: u32 = 3;
/// `MIKAN_INPUT_MOUSE_OTHER_BASE`: extra buttons are `OTHER_BASE + n`.
pub const MOUSE_OTHER_BASE: u32 = 256;

/// `MIKAN_INPUT_WHEEL_HORIZONTAL`, a wheel-binding axis code.
pub const WHEEL_HORIZONTAL: u32 = 0;
/// `MIKAN_INPUT_WHEEL_VERTICAL`, a wheel-binding axis code.
pub const WHEEL_VERTICAL: u32 = 1;

/// `MIKAN_INPUT_OK`: the call succeeded.
pub const OK: c_int = 0;
/// `MIKAN_INPUT_BAD_ARGUMENT`: a null pointer or an invalid value.
pub const BAD_ARGUMENT: c_int = 1;
/// `MIKAN_INPUT_NOT_FOUND`: the unbound input was not bound.
pub const NOT_FOUND: c_int = 2;
/// `MIKAN_INPUT_CONFLICT`: the input is already bound in that context.
pub const CONFLICT: c_int = 3;
/// `MIKAN_INPUT_OVERFLOW`: the event was dropped; the buffer was full.
pub const OVERFLOW: c_int = 4;
/// `MIKAN_INPUT_UNKNOWN_CONTEXT`: the context id is not in the runtime.
pub const UNKNOWN_CONTEXT: c_int = 5;

/// One canonical device event, layout-identical to `mikan_input_event` in
/// `src/mikan_input.h`.
///
/// For key and button events `code` is the key or button code and `x`/`y`
/// are zero; for [`EVENT_MOUSE_MOVE`] they are the absolute cursor position;
/// for [`EVENT_WHEEL`] they are the scroll deltas; for focus events all
/// payload fields are zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    /// Which event this is: one of the [`EVENT_*`] constants.
    pub kind: u32,
    /// The key, button, or wheel-axis code where one applies.
    pub code: u32,
    /// The first payload value (position or delta, see the type).
    pub x: f64,
    /// The second payload value (position or delta, see the type).
    pub y: f64,
}

impl InputEvent {
    /// A key press or release; `pressed` selects the kind.
    #[must_use]
    pub const fn key(code: u32, pressed: bool) -> Self {
        Self {
            kind: if pressed {
                EVENT_KEY_PRESS
            } else {
                EVENT_KEY_RELEASE
            },
            code,
            x: 0.0,
            y: 0.0,
        }
    }

    /// An operating-system repeat of a held key.
    #[must_use]
    pub const fn key_repeat(code: u32) -> Self {
        Self {
            kind: EVENT_KEY_REPEAT,
            code,
            x: 0.0,
            y: 0.0,
        }
    }

    /// A mouse-button press or release; `pressed` selects the kind.
    #[must_use]
    pub const fn mouse_button(code: u32, pressed: bool) -> Self {
        Self {
            kind: if pressed {
                EVENT_MOUSE_PRESS
            } else {
                EVENT_MOUSE_RELEASE
            },
            code,
            x: 0.0,
            y: 0.0,
        }
    }

    /// An absolute cursor move.
    #[must_use]
    pub const fn mouse_move(x: f64, y: f64) -> Self {
        Self {
            kind: EVENT_MOUSE_MOVE,
            code: 0,
            x,
            y,
        }
    }

    /// A wheel scroll by `(dx, dy)`.
    #[must_use]
    pub const fn wheel(dx: f64, dy: f64) -> Self {
        Self {
            kind: EVENT_WHEEL,
            code: 0,
            x: dx,
            y: dy,
        }
    }

    /// The focus-changed event; `focused` selects the kind.
    #[must_use]
    pub const fn focus(focused: bool) -> Self {
        Self {
            kind: if focused {
                EVENT_FOCUS_GAINED
            } else {
                EVENT_FOCUS_LOST
            },
            code: 0,
            x: 0.0,
            y: 0.0,
        }
    }
}

/// An opaque `mikan_input_backend` handle; see [`backend`].
#[repr(C)]
pub struct InputBackend {
    _private: [u8; 0],
}

/// An opaque `mikan_input_runtime` handle; see [`runtime`].
#[repr(C)]
pub struct InputRuntime {
    _private: [u8; 0],
}

unsafe extern "C" {
    /* ---- the C device backend ---- */

    /// Allocates a backend with an event buffer of the given capacity (0
    /// selects the default); returns null on allocation failure.
    pub fn mikan_input_backend_create(capacity: usize) -> *mut InputBackend;

    /// Frees a backend created by [`mikan_input_backend_create`]; null is a
    /// no-op.
    ///
    /// # Safety
    ///
    /// `backend` must be null or a handle from
    /// [`mikan_input_backend_create`] that has not been destroyed; after
    /// this call it must not be used again.
    pub fn mikan_input_backend_destroy(backend: *mut InputBackend);

    /// Feeds one key transition; returns a status code.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live, and must not be accessed
    /// concurrently.
    pub fn mikan_input_backend_push_key(
        backend: *mut InputBackend,
        code: u32,
        pressed: c_int,
        repeat: c_int,
    ) -> c_int;

    /// Feeds one mouse-button transition; returns a status code.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live, and must not be accessed
    /// concurrently.
    pub fn mikan_input_backend_push_mouse_button(
        backend: *mut InputBackend,
        code: u32,
        pressed: c_int,
    ) -> c_int;

    /// Feeds one absolute cursor move; returns a status code.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live, and must not be accessed
    /// concurrently.
    pub fn mikan_input_backend_push_mouse_move(backend: *mut InputBackend, x: f64, y: f64)
    -> c_int;

    /// Feeds one wheel scroll; returns a status code.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live, and must not be accessed
    /// concurrently.
    pub fn mikan_input_backend_push_wheel(backend: *mut InputBackend, dx: f64, dy: f64) -> c_int;

    /// Feeds one focus change; returns a status code.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live, and must not be accessed
    /// concurrently.
    pub fn mikan_input_backend_push_focus(backend: *mut InputBackend, focused: c_int) -> c_int;

    /// 1 if the key is down in the raw device state.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live.
    pub fn mikan_input_backend_is_key_down(backend: *const InputBackend, code: u32) -> c_int;

    /// 1 if the mouse button is down in the raw device state.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live.
    pub fn mikan_input_backend_is_mouse_button_down(
        backend: *const InputBackend,
        code: u32,
    ) -> c_int;

    /// Number of buffered events; 0 for a null backend.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live.
    pub fn mikan_input_backend_len(backend: *const InputBackend) -> usize;

    /// Events dropped over the backend's whole lifetime; 0 for a null
    /// backend.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live.
    pub fn mikan_input_backend_dropped(backend: *const InputBackend) -> usize;

    /// Drains up to `capacity` buffered events into `out_events` and
    /// reports how many were copied.
    ///
    /// # Safety
    ///
    /// `backend` must be live and not accessed concurrently; `out_events`
    /// must be valid for `capacity` writes (`capacity` may be 0);
    /// `out_count` must be a valid writable pointer.
    pub fn mikan_input_backend_read(
        backend: *mut InputBackend,
        out_events: *mut InputEvent,
        capacity: usize,
        out_count: *mut usize,
    ) -> c_int;

    /// Drops every buffered event; device state is untouched.
    ///
    /// # Safety
    ///
    /// `backend` must be live and not accessed concurrently.
    pub fn mikan_input_backend_clear(backend: *mut InputBackend) -> c_int;

    /* ---- the C++ action runtime ---- */

    /// Allocates a runtime; returns null on allocation failure.
    pub fn mikan_input_runtime_create() -> *mut InputRuntime;

    /// Frees a runtime created by [`mikan_input_runtime_create`]; null is a
    /// no-op.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or a handle from [`mikan_input_runtime_create`]
    /// that has not been destroyed; after this call it must not be used
    /// again.
    pub fn mikan_input_runtime_destroy(runtime: *mut InputRuntime);

    /// Adds a context and returns its id; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently;
    /// `out_context` must be a valid writable pointer.
    pub fn mikan_input_runtime_add_context(
        runtime: *mut InputRuntime,
        priority: u32,
        out_context: *mut u32,
    ) -> c_int;

    /// Activates or deactivates a context; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently.
    pub fn mikan_input_runtime_set_context_active(
        runtime: *mut InputRuntime,
        context: u32,
        active: c_int,
    ) -> c_int;

    /// Removes an inactive context; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently.
    pub fn mikan_input_runtime_remove_context(runtime: *mut InputRuntime, context: u32) -> c_int;

    /// Binds one device input to an action inside a context; returns a
    /// status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently.
    pub fn mikan_input_runtime_bind(
        runtime: *mut InputRuntime,
        context: u32,
        device: u32,
        code: u32,
        action: u32,
    ) -> c_int;

    /// Removes one binding; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently.
    pub fn mikan_input_runtime_unbind(
        runtime: *mut InputRuntime,
        context: u32,
        device: u32,
        code: u32,
    ) -> c_int;

    /// Reports a context's binding count; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live; `out_count` must be a valid writable
    /// pointer.
    pub fn mikan_input_runtime_binding_count(
        runtime: *const InputRuntime,
        context: u32,
        out_count: *mut usize,
    ) -> c_int;

    /// Processes one batch of normalized events; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently; `events` must
    /// be valid for `count` reads (`count` may be 0).
    pub fn mikan_input_runtime_process(
        runtime: *mut InputRuntime,
        events: *const InputEvent,
        count: usize,
    ) -> c_int;

    /// Clears per-step edge and wheel state; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live and not accessed concurrently.
    pub fn mikan_input_runtime_end_step(runtime: *mut InputRuntime) -> c_int;

    /// Reads one action's state into `out_state`; returns a status code.
    ///
    /// # Safety
    ///
    /// `runtime` must be live; `out_state` must be a valid writable
    /// pointer.
    pub fn mikan_input_runtime_action_state(
        runtime: *const InputRuntime,
        action: u32,
        out_state: *mut InputActionState,
    ) -> c_int;
}

/// One action's state, layout-identical to `mikan_input_action_state`.
///
/// Prefer [`runtime::Runtime::action_state`], which converts this into the
/// safe [`ActionState`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputActionState {
    /// At least one bound input is currently down.
    pub held: c_int,
    /// The action went from released to held in this step.
    pub just_pressed: c_int,
    /// It went from held to released in this step.
    pub just_released: c_int,
    /// A bound key repeated while held in this step.
    pub repeating: c_int,
    /// 1.0 while held plus the wheel deltas bound to it this step.
    pub value: f32,
}
