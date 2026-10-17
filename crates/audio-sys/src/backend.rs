//! The safe RAII wrapper around the native device backend.
//!
//! [`Backend`] owns one opaque `mikan_audio_backend` handle: created
//! exactly once by [`Backend::create`], destroyed exactly once by its
//! `Drop`, and never exposed as a pointer. Its methods translate the C
//! module's status codes into [`BackendError`] and uphold every
//! precondition the header states — no null handles, buffers sized before
//! the call, exclusive access through `&mut self`.
//!
//! The type is neither `Send` nor `Sync`, matching the header's
//! single-control-thread contract; the raw pointer inside enforces both at
//! compile time. The one `unsafe` entry point is [`Backend::create`]: the
//! render callback and its `user` pointer must outlive the backend, which
//! only the caller can guarantee.

use core::ffi::{CStr, c_void};
use core::ptr::NonNull;

use crate::{
    AudioBackend, AudioBackendDesc, AudioBackendInfo, BACKEND_DEVICE, BACKEND_OFFLINE, CHANNELS,
    DEVICE_ERROR, NO_DEVICE, OK, OUT_OF_MEMORY, RenderFn, mikan_audio_backend_create,
    mikan_audio_backend_destroy, mikan_audio_backend_events, mikan_audio_backend_get_info,
    mikan_audio_backend_is_running, mikan_audio_backend_overruns, mikan_audio_backend_pump,
    mikan_audio_backend_read_s16, mikan_audio_backend_start, mikan_audio_backend_stop,
};

/// Which kind of device a backend drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// A real output device opened through miniaudio.
    Device,
    /// The deterministic offline device, driven by [`Backend::pump`].
    Offline,
}

impl BackendKind {
    /// The raw `MIKAN_AUDIO_BACKEND_*` value.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        match self {
            Self::Device => BACKEND_DEVICE,
            Self::Offline => BACKEND_OFFLINE,
        }
    }

    fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            BACKEND_DEVICE => Some(Self::Device),
            BACKEND_OFFLINE => Some(Self::Offline),
            _ => None,
        }
    }
}

/// A backend operation failed; see the individual methods for which codes
/// they can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// The argument was rejected: a null handle reached the module
    /// (impossible through this wrapper), an invalid pump count, or an
    /// operation only the offline kind supports.
    BadArgument,
    /// No usable output device could be opened (including the case where
    /// miniaudio would have fallen back to its silent null device).
    NoDevice,
    /// The device was opened but failed to start or stop.
    DeviceError,
    /// An allocation inside the module failed.
    OutOfMemory,
}

impl core::fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::BadArgument => "the native backend rejected the argument",
            Self::NoDevice => "no usable audio output device is available",
            Self::DeviceError => "the audio device failed to start or stop",
            Self::OutOfMemory => "the native backend ran out of memory",
        };
        formatter.write_str(message)
    }
}

impl core::error::Error for BackendError {}

fn to_backend_error(status: i32) -> BackendError {
    match status {
        NO_DEVICE => BackendError::NoDevice,
        DEVICE_ERROR => BackendError::DeviceError,
        OUT_OF_MEMORY => BackendError::OutOfMemory,
        _ => BackendError::BadArgument,
    }
}

/// The device-event counters the notification callback maintains; both are
/// zero for the offline kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceEvents {
    /// The device moved to another endpoint (headphones plugged in, ...).
    pub reroutes: u32,
    /// Interruption beginnings (sleep, device grab) the backend saw.
    pub interruptions: u32,
}

/// Read-only backend facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendInfo {
    /// The kind the backend was created with.
    pub kind: BackendKind,
    /// The rate render callbacks actually run at.
    pub sample_rate: u32,
    /// Always [`CHANNELS`]; present for the caller's checks.
    pub channels: u32,
    /// The miniaudio backend name ("ALSA", "WASAPI", ...) or "offline".
    pub backend_name: String,
    /// The opened device name, or "offline".
    pub device_name: String,
}

/// An owning handle to one native device backend.
pub struct Backend {
    handle: NonNull<AudioBackend>,
}

impl Backend {
    /// Creates a backend that drives `render` with `user` as its context.
    ///
    /// For [`BackendKind::Device`] this opens the default output device; if
    /// no real backend initializes, or miniaudio would have fallen back to
    /// its silent null device, the call fails with
    /// [`BackendError::NoDevice`] and the caller decides whether to use
    /// [`BackendKind::Offline`] instead.
    ///
    /// # Safety
    ///
    /// `render` must not call any function of this module, and `user` must
    /// stay valid until this backend and everything `render` reaches
    /// through `user` has been destroyed — with this backend destroyed
    /// first, per the header's shutdown order.
    ///
    /// # Errors
    ///
    /// [`BackendError::NoDevice`] when no device opened,
    /// [`BackendError::OutOfMemory`] when the module's allocation failed,
    /// [`BackendError::BadArgument`] for an invalid rate or block size.
    ///
    /// # Panics
    ///
    /// If the module reports success and returns null, which would be a
    /// module bug.
    pub unsafe fn create(
        kind: BackendKind,
        sample_rate: u32,
        block_frames: u32,
        render: RenderFn,
        user: *mut c_void,
    ) -> Result<Self, BackendError> {
        let desc = AudioBackendDesc {
            kind: kind.as_raw(),
            sample_rate,
            block_frames,
        };
        let mut handle: *mut AudioBackend = core::ptr::null_mut();
        // SAFETY: `desc` is a live local for the call; `render` and `user`
        // follow the contract this function passes through, and `handle`
        // is valid for one write.
        let status = unsafe { mikan_audio_backend_create(&desc, render, user, &mut handle) };
        if status != OK {
            return Err(to_backend_error(status));
        }
        let Some(handle) = NonNull::new(handle) else {
            panic!("mikan-audio-sys: the native backend reported success but returned null");
        };
        Ok(Self { handle })
    }

    /// Starts the device; starting a started backend is a no-op. For the
    /// offline kind this only raises a status flag.
    ///
    /// # Errors
    ///
    /// [`BackendError::DeviceError`] when the device refused to start.
    pub fn start(&mut self) -> Result<(), BackendError> {
        // SAFETY: `self.handle` is live and `&mut self` proves exclusive
        // access, which is the module's threading contract.
        let status = unsafe { mikan_audio_backend_start(self.handle.as_ptr()) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Stops the device; stopping a stopped backend is a no-op.
    ///
    /// # Errors
    ///
    /// [`BackendError::DeviceError`] when the device refused to stop.
    pub fn stop(&mut self) -> Result<(), BackendError> {
        // SAFETY: as `start`.
        let status = unsafe { mikan_audio_backend_stop(self.handle.as_ptr()) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Whether the device is started.
    #[must_use]
    pub fn is_running(&self) -> bool {
        let mut running = 0_u32;
        // SAFETY: the handle is live and the call only reads; the out
        // pointer is a live local.
        let status = unsafe { mikan_audio_backend_is_running(self.handle.as_ptr(), &mut running) };
        debug_assert_eq!(status, OK, "a live handle cannot be rejected");
        running == 1
    }

    /// Copies the backend's facts. The strings are decoded and owned.
    #[must_use]
    pub fn info(&self) -> BackendInfo {
        let mut raw = AudioBackendInfo {
            kind: BACKEND_OFFLINE,
            sample_rate: 0,
            channels: 0,
            backend_name: core::ptr::null(),
            device_name: core::ptr::null(),
        };
        // SAFETY: the handle is live and the call only writes `raw`.
        unsafe { mikan_audio_backend_get_info(self.handle.as_ptr(), &mut raw) };
        let kind = BackendKind::from_raw(raw.kind).unwrap_or(BackendKind::Offline);
        let name = |pointer: *const core::ffi::c_char| -> String {
            if pointer.is_null() {
                String::new()
            } else {
                // SAFETY: the module guarantees both strings are
                // nul-terminated and live until the backend is destroyed.
                unsafe { CStr::from_ptr(pointer) }
                    .to_string_lossy()
                    .into_owned()
            }
        };
        BackendInfo {
            kind,
            sample_rate: raw.sample_rate,
            channels: raw.channels,
            backend_name: name(raw.backend_name),
            device_name: name(raw.device_name),
        }
    }

    /// The device-event counters; see [`DeviceEvents`].
    #[must_use]
    pub fn device_events(&self) -> DeviceEvents {
        let mut reroutes = 0_u32;
        let mut interruptions = 0_u32;
        // SAFETY: the handle is live and the out pointers are live locals.
        let status = unsafe {
            mikan_audio_backend_events(self.handle.as_ptr(), &mut reroutes, &mut interruptions)
        };
        debug_assert_eq!(status, OK, "a live handle cannot be rejected");
        DeviceEvents {
            reroutes,
            interruptions,
        }
    }

    /// Offline kind only: renders `frames` frames through the render
    /// callback into the device-format ring, for [`Backend::read_s16`] to
    /// drain. A block that does not fit is dropped and counted by
    /// [`Backend::overruns`].
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] for the device kind, or when `frames`
    /// is zero or larger than the configured block size.
    pub fn pump(&mut self, frames: u32) -> Result<(), BackendError> {
        // SAFETY: as `start`.
        let status = unsafe { mikan_audio_backend_pump(self.handle.as_ptr(), frames) };
        if status == OK {
            Ok(())
        } else {
            Err(to_backend_error(status))
        }
    }

    /// Offline kind only: drains up to `out.len() / CHANNELS` frames of
    /// interleaved s16 into `out` and returns the frame count drained.
    /// Partial drains are normal; a drain larger than the buffered amount
    /// returns what was buffered.
    ///
    /// # Errors
    ///
    /// [`BackendError::BadArgument`] for the device kind.
    pub fn read_s16(&mut self, out: &mut [i16]) -> Result<usize, BackendError> {
        let capacity = out.len() / CHANNELS as usize;
        let capacity = u32::try_from(capacity).unwrap_or(u32::MAX);
        let mut read = 0_u32;
        // SAFETY: `self.handle` is live and borrowed exclusively; `out` is
        // a live allocation valid for `capacity * CHANNELS` writes, which
        // fits because `capacity` was derived from its length; `read` is a
        // live local.
        let status = unsafe {
            mikan_audio_backend_read_s16(
                self.handle.as_ptr(),
                out.as_mut_ptr(),
                capacity,
                &mut read,
            )
        };
        if status != OK {
            return Err(to_backend_error(status));
        }
        debug_assert!(read <= capacity, "the module never over-reports");
        Ok(read as usize)
    }

    /// Offline kind only: frames dropped over the backend's lifetime
    /// because the ring was full when a pump produced them.
    #[must_use]
    pub fn overruns(&self) -> u32 {
        // SAFETY: the handle is live and the call only reads.
        unsafe { mikan_audio_backend_overruns(self.handle.as_ptr()) }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from `create` and is dropped exactly
        // once, here; no method can use it afterwards. Destroying the
        // backend stops the device and joins its thread, so the render
        // callback cannot run after this point.
        unsafe { mikan_audio_backend_destroy(self.handle.as_ptr()) };
    }
}

impl core::fmt::Debug for Backend {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let info = self.info();
        formatter
            .debug_struct("Backend")
            .field("kind", &info.kind)
            .field("backend_name", &info.backend_name)
            .field("device_name", &info.device_name)
            .field("sample_rate", &info.sample_rate)
            .field("running", &self.is_running())
            .finish()
    }
}
