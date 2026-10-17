//! Raw interface to the native audio module (`mikan_audio`).
//!
//! This crate is the ABI mirror of `src/mikan_audio.h`: the `#[repr(C)]`
//! structs, the status codes, and the `extern "C"` declarations for the
//! two native layers — the C device backend (`audio_backend.c`, built on
//! the vendored miniaudio plus a deterministic offline device) and the C++
//! playback runtime and mixer (`audio_runtime.cpp`).
//! [`system::System`] wires the two behind one safe owner, and
//! `mikan-audio` is the crate applications should depend on: it adds the
//! resource-pipeline and handle semantics on top of these calls.
//!
//! # Why `unsafe` is allowed in this crate
//!
//! The workspace forbids `unsafe_code` crate-wide. This is the crate that
//! bridges into the native audio module, and the rules that make the calls
//! sound — opaque handle validity, buffer lengths, the render callback's
//! lifetime contract, exclusive access to a runtime — are a C ABI contract
//! the Rust type system cannot check. The override is recorded here, per
//! `docs/ffi.md`; every occurrence carries a `// SAFETY:` comment; the
//! safe wrapper (`mikan-audio`) and every consumer contain none.
//!
//! # Layout and ownership
//!
//! [`AudioBackendDesc`], [`AudioPlayDesc`], [`AudioVoice`], and the
//! snapshot structs are plain data read and written by value across the
//! boundary; neither side retains a pointer to them. A [`backend::Backend`]
//! or [`runtime::Runtime`] handle is owned by the caller: the matching
//! `create` allocates it, the matching `destroy` frees it, and each must be
//! destroyed exactly once per successful create — which the RAII wrappers
//! enforce. `mikan_audio_runtime_add_sound` copies the sample buffer; the
//! caller keeps ownership of what it passed.
//!
//! # Threading
//!
//! The module is single-control-thread: every mutating wrapper method takes
//! `&mut self`, and the wrappers are neither `Send` nor `Sync` (the raw
//! pointers inside enforce both). A real device runs its own render thread;
//! the offline pump renders on the caller's. The header documents the full
//! protocol, including the shutdown order [`system::System`] encodes:
//! destroy the backend before the runtime.
//!
//! # The Rust reference
//!
//! [`reference`] is an independent Rust re-implementation of the mixer's
//! arithmetic: linear-interpolation resampling, gain application, and the
//! s16 quantization the offline device applies. It is the test oracle; the
//! native mixer's output is cross-checked against it bit for bit on
//! deterministic voices. That is a requirement of the module, not
//! duplication for its own sake.

use core::ffi::{c_char, c_int, c_void};

pub mod backend;
pub mod reference;
pub mod runtime;
pub mod system;

pub use backend::{Backend, BackendError, BackendInfo, BackendKind};
pub use runtime::{PlayDesc, Runtime, RuntimeError, Sound, Stats, Voice, VoiceState, VoiceStatus};
pub use system::{AudioError, System};

/// `MIKAN_AUDIO_CHANNELS`: the mixer's output channel count.
pub const CHANNELS: u32 = 2;
/// `MIKAN_AUDIO_DEFAULT_SAMPLE_RATE`.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// `MIKAN_AUDIO_MAX_SOUNDS`: concurrent registered sounds.
pub const MAX_SOUNDS: u32 = 256;
/// `MIKAN_AUDIO_MAX_VOICES`: concurrent voices.
pub const MAX_VOICES: u32 = 64;
/// `MIKAN_AUDIO_BUS_COUNT`.
pub const BUS_COUNT: u32 = 2;
/// `MIKAN_AUDIO_QUEUE_CAPACITY`: outstanding control commands.
pub const QUEUE_CAPACITY: u32 = 256;
/// `MIKAN_AUDIO_MIN_PITCH`.
pub const MIN_PITCH: f32 = 0.25;
/// `MIKAN_AUDIO_MAX_PITCH`.
pub const MAX_PITCH: f32 = 4.0;

/// `MIKAN_AUDIO_OK`: the call succeeded.
pub const OK: c_int = 0;
/// `MIKAN_AUDIO_BAD_ARGUMENT`.
pub const BAD_ARGUMENT: c_int = 1;
/// `MIKAN_AUDIO_DEVICE_ERROR`.
pub const DEVICE_ERROR: c_int = 2;
/// `MIKAN_AUDIO_NO_DEVICE`.
pub const NO_DEVICE: c_int = 3;
/// `MIKAN_AUDIO_CAPACITY`.
pub const CAPACITY: c_int = 4;
/// `MIKAN_AUDIO_NOT_FOUND`.
pub const NOT_FOUND: c_int = 5;
/// `MIKAN_AUDIO_UNSUPPORTED`.
pub const UNSUPPORTED: c_int = 6;
/// `MIKAN_AUDIO_BUSY`.
pub const BUSY: c_int = 7;
/// `MIKAN_AUDIO_OUT_OF_MEMORY`.
pub const OUT_OF_MEMORY: c_int = 8;

/// `MIKAN_AUDIO_BACKEND_DEVICE`: a real device through miniaudio.
pub const BACKEND_DEVICE: u32 = 0;
/// `MIKAN_AUDIO_BACKEND_OFFLINE`: the deterministic offline device.
pub const BACKEND_OFFLINE: u32 = 1;

/// `MIKAN_AUDIO_BUS_SFX`.
pub const BUS_SFX: u32 = 0;
/// `MIKAN_AUDIO_BUS_MUSIC`.
pub const BUS_MUSIC: u32 = 1;

/// `MIKAN_AUDIO_VOICE_PLAYING`.
pub const VOICE_PLAYING: u32 = 1;
/// `MIKAN_AUDIO_VOICE_PAUSED`.
pub const VOICE_PAUSED: u32 = 2;
/// `MIKAN_AUDIO_VOICE_FINISHED`.
pub const VOICE_FINISHED: u32 = 3;

/// Opaque `mikan_audio_backend`; never dereferenced from Rust.
#[repr(C)]
pub struct AudioBackend {
    _private: [u8; 0],
}

/// Opaque `mikan_audio_runtime`; never dereferenced from Rust.
#[repr(C)]
pub struct AudioRuntime {
    _private: [u8; 0],
}

/// The render callback type the backend drives; layout-identical to
/// `mikan_audio_render_fn`.
pub type RenderFn = unsafe extern "C" fn(user: *mut c_void, frames: *mut f32, frame_count: u32);

/// Backend creation parameters, layout-identical to
/// `mikan_audio_backend_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioBackendDesc {
    /// One of [`BACKEND_DEVICE`] or [`BACKEND_OFFLINE`].
    pub kind: u32,
    /// Render rate; zero selects [`DEFAULT_SAMPLE_RATE`].
    pub sample_rate: u32,
    /// Frames per render call; zero selects 1024.
    pub block_frames: u32,
}

/// Read-only backend facts, layout-identical to
/// `mikan_audio_backend_info`. The strings are owned by the backend and
/// stay valid until it is destroyed.
#[repr(C)]
pub struct AudioBackendInfo {
    /// One of [`BACKEND_DEVICE`] or [`BACKEND_OFFLINE`].
    pub kind: u32,
    /// The rate render callbacks run at.
    pub sample_rate: u32,
    /// Always [`CHANNELS`].
    pub channels: u32,
    /// The miniaudio backend name ("ALSA", ...) or "offline".
    pub backend_name: *const c_char,
    /// The opened device name, or "offline".
    pub device_name: *const c_char,
}

/// A voice handle, layout-identical to `mikan_audio_voice`; only
/// `mikan_audio_runtime_play` mints valid ones.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioVoice {
    /// Slot index, 0..[`MAX_VOICES`].
    pub index: u32,
    /// Slot generation; zero never names a voice.
    pub generation: u32,
}

/// A sound handle, layout-identical to `mikan_audio_sound`; only
/// `mikan_audio_runtime_add_sound` mints valid ones.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioSound {
    /// Slot index, 0..[`MAX_SOUNDS`].
    pub index: u32,
    /// Slot generation; zero never names a sound.
    pub generation: u32,
}

/// A voice state snapshot, layout-identical to
/// `mikan_audio_voice_state`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioVoiceState {
    /// One of [`VOICE_PLAYING`], [`VOICE_PAUSED`], [`VOICE_FINISHED`].
    pub state: u32,
    /// The sound the voice plays; generation zero for none.
    pub sound: AudioSound,
    /// 1 when the voice loops.
    pub looping: u32,
    /// Gain, 0..1.
    pub volume: f32,
    /// Pitch factor, [`MIN_PITCH`]..[`MAX_PITCH`].
    pub pitch: f32,
    /// Frames consumed of the sound.
    pub position_frames: u64,
}

/// One `play` request, layout-identical to `mikan_audio_play_desc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioPlayDesc {
    /// A live sound handle from `add_sound`.
    pub sound: AudioSound,
    /// One of [`BUS_SFX`] or [`BUS_MUSIC`].
    pub bus: u32,
    /// Non-zero to loop.
    pub looping: u32,
    /// Gain in 0..1; clamped.
    pub volume: f32,
    /// Pitch factor; clamped to the documented range.
    pub pitch: f32,
}

/// Runtime counters, layout-identical to `mikan_audio_runtime_stats`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStats {
    /// Voices that began playing.
    pub voices_started: u64,
    /// Voices that reached a terminal state after playing.
    pub voices_finished: u64,
    /// Render blocks the mixer produced.
    pub blocks_mixed: u64,
    /// Commands the render side applied.
    pub commands_applied: u64,
    /// Control calls rejected because the command queue was full.
    pub commands_rejected: u64,
    /// Sounds fully retired.
    pub sounds_retired: u64,
    /// Live voices stopped because their sound was removed.
    pub voices_orphaned: u64,
}

unsafe extern "C" {
    /* ---- the C device backend ---- */

    /// Allocates and opens a backend; see the header for the contract.
    ///
    /// # Safety
    ///
    /// `render` must never call back into the backend, and `user` must stay
    /// valid until the matching destroy.
    pub fn mikan_audio_backend_create(
        desc: *const AudioBackendDesc,
        render: RenderFn,
        user: *mut c_void,
        out_backend: *mut *mut AudioBackend,
    ) -> c_int;

    /// Stops the device, joins its thread, and frees the backend; null is a
    /// no-op.
    ///
    /// # Safety
    ///
    /// `backend` must be null or a handle from
    /// [`mikan_audio_backend_create`] that has not been destroyed.
    pub fn mikan_audio_backend_destroy(backend: *mut AudioBackend);

    /// Starts the device.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live and not accessed concurrently.
    pub fn mikan_audio_backend_start(backend: *mut AudioBackend) -> c_int;

    /// Stops the device.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_backend_start`].
    pub fn mikan_audio_backend_stop(backend: *mut AudioBackend) -> c_int;

    /// Copies the backend's facts into `out_info`.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live; `out_info` must be valid for one
    /// write.
    pub fn mikan_audio_backend_get_info(
        backend: *const AudioBackend,
        out_info: *mut AudioBackendInfo,
    );

    /// Writes 1 when the device is started, else 0.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live; `out_running` must be valid for one
    /// write. Safe to call from any thread.
    pub fn mikan_audio_backend_is_running(
        backend: *const AudioBackend,
        out_running: *mut u32,
    ) -> c_int;

    /// Reads the device-event counters.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_backend_is_running`] for both out pointers.
    pub fn mikan_audio_backend_events(
        backend: *const AudioBackend,
        out_reroutes: *mut u32,
        out_interruptions: *mut u32,
    ) -> c_int;

    /// Offline kind only: renders `frames` frames into the output ring.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live and not accessed concurrently.
    pub fn mikan_audio_backend_pump(backend: *mut AudioBackend, frames: u32) -> c_int;

    /// Offline kind only: drains up to `frame_capacity` frames of s16.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_backend_pump`]; `out_frames` must be valid for
    /// `frame_capacity * CHANNELS` writes and `out_read` for one.
    pub fn mikan_audio_backend_read_s16(
        backend: *mut AudioBackend,
        out_frames: *mut i16,
        frame_capacity: u32,
        out_read: *mut u32,
    ) -> c_int;

    /// Offline kind only: frames dropped by overruns.
    ///
    /// # Safety
    ///
    /// `backend` must be null or live.
    pub fn mikan_audio_backend_overruns(backend: *const AudioBackend) -> u32;

    /* ---- the C++ runtime ---- */

    /// Creates a runtime mixing at `sample_rate`.
    ///
    /// # Safety
    ///
    /// `out_runtime` must be valid for one write.
    pub fn mikan_audio_runtime_create(
        sample_rate: u32,
        out_runtime: *mut *mut AudioRuntime,
    ) -> c_int;

    /// Destroys a runtime and every buffer it owns; null is a no-op.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or a handle from
    /// [`mikan_audio_runtime_create`] that has not been destroyed, and no
    /// render callback may be running into it.
    pub fn mikan_audio_runtime_destroy(runtime: *mut AudioRuntime);

    /// Registers a sound by copying `frame_count * channels` samples.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently;
    /// `samples` must be valid for `frame_count * channels` reads;
    /// `out_sound` for one write.
    pub fn mikan_audio_runtime_add_sound(
        runtime: *mut AudioRuntime,
        samples: *const f32,
        frame_count: u32,
        channels: u32,
        sample_rate: u32,
        out_sound: *mut AudioSound,
    ) -> c_int;

    /// Retires a sound.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently.
    pub fn mikan_audio_runtime_remove_sound(runtime: *mut AudioRuntime, sound: AudioSound)
    -> c_int;

    /// Frees the buffers the render side acknowledged.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently.
    pub fn mikan_audio_runtime_collect(runtime: *mut AudioRuntime);

    /// Writes 1 when `sound` is live, else 0.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live; `out_live` for one write. Safe from
    /// any thread.
    pub fn mikan_audio_runtime_sound_live(
        runtime: *const AudioRuntime,
        sound: AudioSound,
        out_live: *mut u32,
    ) -> c_int;

    /// Allocates a voice for `desc`.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently;
    /// `desc` must be valid for one read; `out_voice` for one write.
    pub fn mikan_audio_runtime_play(
        runtime: *mut AudioRuntime,
        desc: *const AudioPlayDesc,
        out_voice: *mut AudioVoice,
    ) -> c_int;

    /// Pauses a live voice.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently.
    pub fn mikan_audio_runtime_pause(runtime: *mut AudioRuntime, voice: AudioVoice) -> c_int;

    /// Resumes a paused voice.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_runtime_pause`].
    pub fn mikan_audio_runtime_resume(runtime: *mut AudioRuntime, voice: AudioVoice) -> c_int;

    /// Stops a live voice, resetting its position.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_runtime_pause`].
    pub fn mikan_audio_runtime_stop(runtime: *mut AudioRuntime, voice: AudioVoice) -> c_int;

    /// Sets a voice's gain.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_runtime_pause`].
    pub fn mikan_audio_runtime_set_voice_volume(
        runtime: *mut AudioRuntime,
        voice: AudioVoice,
        volume: f32,
    ) -> c_int;

    /// Sets a voice's pitch.
    ///
    /// # Safety
    ///
    /// As [`mikan_audio_runtime_pause`].
    pub fn mikan_audio_runtime_set_voice_pitch(
        runtime: *mut AudioRuntime,
        voice: AudioVoice,
        pitch: f32,
    ) -> c_int;

    /// Sets one bus's gain.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently.
    pub fn mikan_audio_runtime_set_bus_volume(
        runtime: *mut AudioRuntime,
        bus: u32,
        volume: f32,
    ) -> c_int;

    /// Sets the master gain.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live and not accessed concurrently.
    pub fn mikan_audio_runtime_set_master_volume(runtime: *mut AudioRuntime, volume: f32) -> c_int;

    /// Reads a voice's state.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live; `out_state` for one write. Safe from
    /// any thread.
    pub fn mikan_audio_runtime_voice_state(
        runtime: *const AudioRuntime,
        voice: AudioVoice,
        out_state: *mut AudioVoiceState,
    ) -> c_int;

    /// Copies the runtime's counters.
    ///
    /// # Safety
    ///
    /// `runtime` must be null or live; `out_stats` for one write. Safe from
    /// any thread.
    pub fn mikan_audio_runtime_stats_into(runtime: *const AudioRuntime, out_stats: *mut AudioStats);

    /// The render entry point the backend is given; callable directly to
    /// drive the mixer deterministically.
    ///
    /// # Safety
    ///
    /// `user` must be a `mikan_audio_runtime *` that outlives the call;
    /// `frames` must be valid for `frame_count * CHANNELS` writes.
    pub fn mikan_audio_runtime_render(user: *mut c_void, frames: *mut f32, frame_count: u32);
}
