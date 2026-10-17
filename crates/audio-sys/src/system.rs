//! The safe owner that wires the C device backend to the C++ runtime.
//!
//! [`System`] owns both native handles and encodes the module's shutdown
//! order in its field order: `backend` is declared first, so it is dropped
//! first — stopping the device and joining its thread — and only then is
//! the runtime freed. The render callback the backend drives is the
//! module's own mixer entry point, `mikan_audio_runtime_render`, with the
//! runtime handle as its context; that wiring is the one `unsafe` block
//! this type contains, and its soundness is exactly the field order above.
//!
//! Applications should use `mikan-audio`, which adds the resource
//! pipeline, ergonomic handles, and errors on top of this type. This
//! layer is the smallest safe wrapper that *can* be built: every raw
//! pointer, every status code, and every lifetime decision stays inside
//! it.

use crate::backend::{Backend, BackendError, BackendInfo, BackendKind, DeviceEvents};
use crate::runtime::{Runtime, RuntimeError};
use crate::{DEFAULT_SAMPLE_RATE, RenderFn, mikan_audio_runtime_render};

/// A whole-module operation failed; this flattens [`BackendError`] and
/// [`RuntimeError`] into the one error type callers of a wired system see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    /// No usable output device could be opened.
    NoDevice,
    /// The device was opened but a device operation failed.
    DeviceError,
    /// The sample rate was outside the documented range.
    InvalidSampleRate,
    /// An argument was rejected by one of the layers.
    BadArgument,
    /// The data is outside the module's documented support.
    Unsupported,
    /// A pool is exhausted (voices or sound slots).
    Capacity,
    /// A sound or voice handle does not name a live object.
    NotFound,
    /// The command queue is full; retry after the next render block.
    Busy,
    /// An allocation inside the module failed.
    OutOfMemory,
}

impl core::fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::NoDevice => "no usable audio output device is available",
            Self::DeviceError => "the audio device failed to start or stop",
            Self::InvalidSampleRate => "the sample rate is outside 8000..=192000",
            Self::BadArgument => "the native audio module rejected the argument",
            Self::Unsupported => "the data is outside the audio module's support",
            Self::Capacity => "no audio voice or sound slot is free",
            Self::NotFound => "the sound or voice is not live",
            Self::Busy => "the audio command queue is full; retry after the next render block",
            Self::OutOfMemory => "the native audio module ran out of memory",
        };
        formatter.write_str(message)
    }
}

impl core::error::Error for AudioError {}

impl From<BackendError> for AudioError {
    fn from(error: BackendError) -> Self {
        match error {
            BackendError::NoDevice => Self::NoDevice,
            BackendError::DeviceError => Self::DeviceError,
            BackendError::BadArgument => Self::BadArgument,
            BackendError::OutOfMemory => Self::OutOfMemory,
        }
    }
}

impl From<RuntimeError> for AudioError {
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::InvalidSampleRate => Self::InvalidSampleRate,
            RuntimeError::BadArgument => Self::BadArgument,
            RuntimeError::Unsupported => Self::Unsupported,
            RuntimeError::Capacity => Self::Capacity,
            RuntimeError::NotFound => Self::NotFound,
            RuntimeError::Busy => Self::Busy,
            RuntimeError::OutOfMemory => Self::OutOfMemory,
        }
    }
}

/// One device backend wired to one playback runtime.
///
/// Create it with [`System::new_device`] or [`System::new_offline`]; reach
/// the two halves with [`System::backend`]/[`System::runtime`]. Control
/// calls are single-threaded by construction: the mutating accessors take
/// `&mut self`, and the raw pointers inside make the whole type neither
/// `Send` nor `Sync`.
pub struct System {
    // Field order is the shutdown order: the backend (which owns the
    // thread that renders into the runtime) drops first, then the runtime.
    backend: Backend,
    runtime: Runtime,
}

impl System {
    /// Opens the default output device and wires it to a runtime mixing at
    /// `sample_rate` (0 selects [`DEFAULT_SAMPLE_RATE`]).
    ///
    /// # Errors
    ///
    /// [`AudioError::NoDevice`] when no real backend initialized (including
    /// the case where miniaudio would have fallen back to its silent null
    /// device); the caller decides whether [`System::new_offline`] is the
    /// right answer for that machine. Also [`AudioError::InvalidSampleRate`]
    /// and [`AudioError::OutOfMemory`].
    pub fn new_device(sample_rate: u32, block_frames: u32) -> Result<Self, AudioError> {
        Self::with_kind(BackendKind::Device, sample_rate, block_frames)
    }

    /// Creates the deterministic offline device and wires it to a runtime
    /// mixing at `sample_rate` (0 selects [`DEFAULT_SAMPLE_RATE`]).
    /// Nothing here depends on the machine having audio hardware: the
    /// caller drives rendering with [`System::pump`] and drains the
    /// device-format output with [`System::read_s16`].
    ///
    /// # Errors
    ///
    /// [`AudioError::InvalidSampleRate`] and [`AudioError::OutOfMemory`].
    pub fn new_offline(sample_rate: u32, block_frames: u32) -> Result<Self, AudioError> {
        Self::with_kind(BackendKind::Offline, sample_rate, block_frames)
    }

    fn with_kind(
        kind: BackendKind,
        sample_rate: u32,
        block_frames: u32,
    ) -> Result<Self, AudioError> {
        let sample_rate = if sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        let runtime = Runtime::new(sample_rate)?;
        let render: RenderFn = mikan_audio_runtime_render;
        // SAFETY: `render` is the module's own mixer entry point and never
        // calls back into the backend; `user` is the runtime handle, and
        // `System`'s field order keeps the runtime alive until after the
        // backend — dropping the backend joins the device thread, so the
        // pointer is never used after the runtime is freed.
        let backend = unsafe {
            Backend::create(
                kind,
                sample_rate,
                block_frames,
                render,
                runtime.as_ptr().cast(),
            )?
        };
        debug_assert_eq!(
            backend.info().sample_rate,
            runtime.sample_rate(),
            "the backend renders at the rate the runtime mixes at"
        );
        Ok(Self { backend, runtime })
    }

    /// Starts the device. For the offline kind this only raises a status
    /// flag: pumping is always what renders.
    ///
    /// # Errors
    ///
    /// [`AudioError::DeviceError`] when the device refused to start.
    pub fn start(&mut self) -> Result<(), AudioError> {
        Ok(self.backend.start()?)
    }

    /// Stops the device; stopping a stopped system is a no-op.
    ///
    /// # Errors
    ///
    /// [`AudioError::DeviceError`] when the device refused to stop.
    pub fn stop(&mut self) -> Result<(), AudioError> {
        Ok(self.backend.stop()?)
    }

    /// Whether the device is started.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.backend.is_running()
    }

    /// The backend's facts (kind, negotiated rate, names).
    #[must_use]
    pub fn info(&self) -> BackendInfo {
        self.backend.info()
    }

    /// The device-event counters; always zero for the offline kind.
    #[must_use]
    pub fn device_events(&self) -> DeviceEvents {
        self.backend.device_events()
    }

    /// Offline kind only: renders `frames` frames and appends the
    /// device-format s16 block to the output ring.
    ///
    /// # Errors
    ///
    /// [`AudioError::BadArgument`] for the device kind or an out-of-range
    /// count.
    pub fn pump(&mut self, frames: u32) -> Result<(), AudioError> {
        Ok(self.backend.pump(frames)?)
    }

    /// Offline kind only: drains up to `out.len() / CHANNELS` frames of
    /// interleaved s16 and returns the frame count drained.
    ///
    /// # Errors
    ///
    /// [`AudioError::BadArgument`] for the device kind.
    pub fn read_s16(&mut self, out: &mut [i16]) -> Result<usize, AudioError> {
        Ok(self.backend.read_s16(out)?)
    }

    /// Offline kind only: frames dropped by overruns.
    #[must_use]
    pub fn overruns(&self) -> u32 {
        self.backend.overruns()
    }

    /// The runtime half.
    #[must_use]
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The runtime half, mutably.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// The backend half.
    #[must_use]
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// The backend half, mutably.
    pub fn backend_mut(&mut self) -> &mut Backend {
        &mut self.backend
    }
}

impl core::fmt::Debug for System {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("System")
            .field("info", &self.info())
            .field("running", &self.is_running())
            .field("stats", &self.runtime.stats())
            .finish()
    }
}
