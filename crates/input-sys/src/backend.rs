//! The safe RAII wrapper around the native device backend.
//!
//! [`Backend`] owns one opaque `mikan_input_backend` handle: created exactly
//! once by [`Backend::new`], destroyed exactly once by its `Drop`, and never
//! exposed as a pointer. Its methods translate the C module's status codes
//! into [`BackendError`] and uphold every precondition the header states —
//! no null handles, buffers sized before the call, exclusive access through
//! `&mut self`.
//!
//! The type is neither `Send` nor `Sync`, matching the header's single-thread
//! contract; the raw pointer inside enforces both at compile time.

use core::ptr::NonNull;

use crate::{
    InputBackend, InputEvent, OK, OVERFLOW, mikan_input_backend_clear, mikan_input_backend_create,
    mikan_input_backend_destroy, mikan_input_backend_dropped, mikan_input_backend_is_key_down,
    mikan_input_backend_is_mouse_button_down, mikan_input_backend_len,
    mikan_input_backend_push_focus, mikan_input_backend_push_key,
    mikan_input_backend_push_mouse_button, mikan_input_backend_push_mouse_move,
    mikan_input_backend_push_wheel, mikan_input_backend_read,
};

/// A backend operation failed; see the individual methods for which codes
/// they can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// The argument was rejected: a null handle reached the module (impossible
    /// through this wrapper), or an allocation inside the module failed.
    BadArgument,
    /// The event was produced but the buffer was full, so it was dropped and
    /// counted. The stream stays consistent; the event is lost.
    Overflow,
}

fn to_backend_error(status: i32) -> BackendError {
    if status == OVERFLOW {
        BackendError::Overflow
    } else {
        BackendError::BadArgument
    }
}

/// An owning handle to one native device backend.
pub struct Backend {
    handle: NonNull<InputBackend>,
}

impl Backend {
    /// Creates a backend with an event buffer of `capacity` events; zero
    /// selects the module's default (4096).
    ///
    /// # Panics
    ///
    /// If the native allocation fails — the same failure mode as any other
    /// allocation, reported where it happens rather than threaded through
    /// every caller as a `Result`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        // SAFETY: `create` takes no preconditions and returns either null or
        // a fresh, exclusively-owned handle, which `NonNull::new` filters and
        // `Drop` frees exactly once.
        let handle = unsafe { mikan_input_backend_create(capacity) };
        let Some(handle) = NonNull::new(handle) else {
            panic!("mikan-input-sys: the native input backend allocation failed");
        };
        Self { handle }
    }

    /// Feeds one key transition. `repeat` marks an operating-system repeat.
    ///
    /// # Errors
    ///
    /// [`BackendError::Overflow`] if the event did not fit the buffer.
    pub fn push_key(&mut self, code: u32, pressed: bool, repeat: bool) -> Result<(), BackendError> {
        // SAFETY: `self.handle` is live and `&mut self` proves exclusive
        // access, which is the module's threading contract.
        let status = unsafe {
            mikan_input_backend_push_key(self.handle.as_ptr(), code, pressed as i32, repeat as i32)
        };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Feeds one mouse-button transition.
    ///
    /// # Errors
    ///
    /// [`BackendError::Overflow`] if the event did not fit the buffer.
    pub fn push_mouse_button(&mut self, code: u32, pressed: bool) -> Result<(), BackendError> {
        // SAFETY: as `push_key`.
        let status = unsafe {
            mikan_input_backend_push_mouse_button(self.handle.as_ptr(), code, pressed as i32)
        };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Feeds one absolute cursor move. Consecutive moves are coalesced by
    /// the module; only the latest position is buffered.
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] if a coordinate is not finite.
    pub fn push_mouse_move(&mut self, x: f64, y: f64) -> Result<(), BackendError> {
        // SAFETY: as `push_key`.
        let status = unsafe { mikan_input_backend_push_mouse_move(self.handle.as_ptr(), x, y) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Feeds one wheel scroll. Consecutive scrolls are coalesced by summing
    /// their deltas.
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] if a delta is not finite.
    pub fn push_wheel(&mut self, dx: f64, dy: f64) -> Result<(), BackendError> {
        // SAFETY: as `push_key`.
        let status = unsafe { mikan_input_backend_push_wheel(self.handle.as_ptr(), dx, dy) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Feeds one focus change. Losing focus makes the module emit one
    /// release per held device, then a focus-lost event, and clear its raw
    /// device state.
    ///
    /// # Errors
    ///
    /// [`BackendError::Overflow`] if an event did not fit the buffer.
    pub fn push_focus(&mut self, focused: bool) -> Result<(), BackendError> {
        // SAFETY: as `push_key`.
        let status =
            unsafe { mikan_input_backend_push_focus(self.handle.as_ptr(), focused as i32) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Whether the key is down in the raw device state.
    #[must_use]
    pub fn is_key_down(&self, code: u32) -> bool {
        // SAFETY: `self.handle` is live for as long as `self` is; the call
        // only reads.
        unsafe { mikan_input_backend_is_key_down(self.handle.as_ptr(), code) == 1 }
    }

    /// Whether the mouse button is down in the raw device state.
    #[must_use]
    pub fn is_mouse_button_down(&self, code: u32) -> bool {
        // SAFETY: as `is_key_down`.
        unsafe { mikan_input_backend_is_mouse_button_down(self.handle.as_ptr(), code) == 1 }
    }

    /// Number of buffered events.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: as `is_key_down`.
        unsafe { mikan_input_backend_len(self.handle.as_ptr()) }
    }

    /// Whether no events are buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Events dropped over the backend's whole lifetime.
    #[must_use]
    pub fn dropped(&self) -> usize {
        // SAFETY: as `is_key_down`.
        unsafe { mikan_input_backend_dropped(self.handle.as_ptr()) }
    }

    /// Drains every buffered event into `out`, which is cleared first and
    /// then extended in stream order.
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn drain(&mut self, out: &mut Vec<InputEvent>) -> Result<(), BackendError> {
        out.clear();
        let mut count = 0_usize;
        // Drain in one pass: `out` is sized to the buffered count before the
        // call, which is the module's maximum report.
        out.resize(self.len(), InputEvent::key(0, false));
        // SAFETY: `self.handle` is live and borrowed exclusively; `out` is a
        // live allocation valid for `out.len()` writes; `count` is a live
        // local the module writes.
        let status = unsafe {
            mikan_input_backend_read(
                self.handle.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                &mut count,
            )
        };
        if status != OK {
            return Err(to_backend_error(status));
        }
        debug_assert!(count <= out.len(), "the module never over-reports");
        out.truncate(count);
        Ok(())
    }

    /// Drops every buffered event; the raw device state is untouched.
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] if the module rejects the call, which
    /// this wrapper's preconditions make unreachable.
    pub fn clear(&mut self) -> Result<(), BackendError> {
        // SAFETY: as `push_key`.
        let status = unsafe { mikan_input_backend_clear(self.handle.as_ptr()) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }
}

impl Default for Backend {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from `create` and is dropped exactly
        // once, here; no method can use it afterwards.
        unsafe { mikan_input_backend_destroy(self.handle.as_ptr()) };
    }
}

impl core::fmt::Debug for Backend {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Backend")
            .field("buffered", &self.len())
            .field("dropped", &self.dropped())
            .finish()
    }
}
