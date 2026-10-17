//! The safe RAII wrapper around the native playback runtime and mixer.
//!
//! [`Runtime`] owns one opaque `mikan_audio_runtime` handle: created
//! exactly once by [`Runtime::new`], destroyed exactly once by its `Drop`,
//! and never exposed as a pointer. The methods encode the module's
//! lifetime rules in the type system:
//!
//! - mutating calls take `&mut self`, so the control side is single-threaded
//!   by construction — exactly the producer the command queue requires;
//! - [`Voice`] handles are plain `(index, generation)` pairs that only
//!   [`Runtime::play`] mints; a stale handle reports [`VoiceState::Finished`]
//!   rather than naming whatever voice recycled the slot, and a forged
//!   handle is rejected with [`RuntimeError::NotFound`];
//! - the runtime is neither `Send` nor `Sync` (the raw pointer inside
//!   enforces both); the native side is designed to be *rendered* from
//!   another thread, but that thread is the backend's, wired by
//!   [`crate::system::System`], never this type crossing threads.
//!
//! `docs/audio.md` holds the playback semantics — what stop does to the
//! position, when volume changes apply, how capacity exhaustion reports
//! itself — and this file keeps the wrapper a pure translation layer over
//! them.

use core::ptr::NonNull;

use crate::{
    AudioPlayDesc, AudioRuntime, AudioSound, AudioStats, AudioVoice, AudioVoiceState, BAD_ARGUMENT,
    BUS_SFX, BUSY, CAPACITY, NOT_FOUND, OK, OUT_OF_MEMORY, UNSUPPORTED, VOICE_FINISHED,
    VOICE_PAUSED, VOICE_PLAYING, mikan_audio_runtime_add_sound, mikan_audio_runtime_collect,
    mikan_audio_runtime_create, mikan_audio_runtime_destroy, mikan_audio_runtime_pause,
    mikan_audio_runtime_play, mikan_audio_runtime_remove_sound, mikan_audio_runtime_resume,
    mikan_audio_runtime_set_bus_volume, mikan_audio_runtime_set_master_volume,
    mikan_audio_runtime_set_voice_pitch, mikan_audio_runtime_set_voice_volume,
    mikan_audio_runtime_sound_live, mikan_audio_runtime_stats_into, mikan_audio_runtime_stop,
    mikan_audio_runtime_voice_state,
};

/// A runtime operation failed; see the individual methods for which codes
/// they can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// The sample rate was outside the documented 8000..=192000 range.
    InvalidSampleRate,
    /// The argument was rejected: a buffer length that disagrees with its
    /// channel count, a non-finite float, or a bus outside its range.
    BadArgument,
    /// The data is outside the module's documented support: a sound with a
    /// channel count other than 1 or 2.
    Unsupported,
    /// A pool is exhausted: no free sound slot, or no free voice.
    Capacity,
    /// The sound or voice handle does not name a live object.
    NotFound,
    /// The command queue is full; retry after the next render block.
    Busy,
    /// An allocation inside the module failed.
    OutOfMemory,
}

impl core::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::InvalidSampleRate => "the sample rate is outside 8000..=192000",
            Self::BadArgument => "the native runtime rejected the argument",
            Self::Unsupported => "the sound's channel count is not 1 or 2",
            Self::Capacity => "no voice or sound slot is free",
            Self::NotFound => "the sound or voice is not live",
            Self::Busy => "the command queue is full; retry after the next render block",
            Self::OutOfMemory => "the native runtime ran out of memory",
        };
        formatter.write_str(message)
    }
}

impl core::error::Error for RuntimeError {}

fn to_runtime_error(status: i32) -> RuntimeError {
    match status {
        BAD_ARGUMENT => RuntimeError::BadArgument,
        CAPACITY => RuntimeError::Capacity,
        NOT_FOUND => RuntimeError::NotFound,
        UNSUPPORTED => RuntimeError::Unsupported,
        BUSY => RuntimeError::Busy,
        OUT_OF_MEMORY => RuntimeError::OutOfMemory,
        _ => RuntimeError::BadArgument,
    }
}

fn status_to_unit(status: i32) -> Result<(), RuntimeError> {
    if status == OK {
        Ok(())
    } else {
        Err(to_runtime_error(status))
    }
}

/// One playing (or finished) voice, as minted by [`Runtime::play`].
///
/// The pair of fields is opaque on purpose: consumers hold the value, pass
/// it back to the runtime, and never synthesize one. `generation` is what
/// makes a recycled slot detectable, so a handle to a finished voice can
/// never name a later, unrelated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Voice {
    index: u32,
    generation: u32,
}

impl Voice {
    const fn to_raw(self) -> AudioVoice {
        AudioVoice {
            index: self.index,
            generation: self.generation,
        }
    }
}

/// One registered sound, as minted by [`Runtime::add_sound`].
///
/// Like [`Voice`], the pair of fields is opaque and generation-checked: a
/// handle that outlives its [`Runtime::remove_sound`] call can never name
/// whatever sound later reuses the slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sound {
    index: u32,
    generation: u32,
}

impl Sound {
    const fn to_raw(self) -> AudioSound {
        AudioSound {
            index: self.index,
            generation: self.generation,
        }
    }

    const fn from_raw(raw: AudioSound) -> Option<Self> {
        if raw.generation == 0 {
            None
        } else {
            Some(Self {
                index: raw.index,
                generation: raw.generation,
            })
        }
    }
}

/// What a voice is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// Mixing (or requested and about to mix at the next block boundary).
    Playing,
    /// Paused; the position is preserved.
    Paused,
    /// Terminal: completed, stopped, or released because its sound was
    /// removed. A stale handle — one whose slot was recycled — reports
    /// this too, because the voice it named is over.
    Finished,
}

/// A voice's visible state: [`VoiceState`] plus its last published
/// position (frames consumed; a stop resets it to zero, a natural
/// completion leaves it at the sound's frame count).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceStatus {
    /// What the voice is doing.
    pub state: VoiceState,
    /// The sound the voice plays; `None` for a slot that never played.
    pub sound: Option<Sound>,
    /// Whether the voice loops.
    pub looping: bool,
    /// The voice's gain, 0..1.
    pub volume: f32,
    /// The voice's pitch factor, [`crate::MIN_PITCH`]..[`crate::MAX_PITCH`].
    pub pitch: f32,
    /// Frames consumed of the sound.
    pub position_frames: u64,
}

/// Everything [`Runtime::play`] needs to start one voice.
///
/// Build one from [`PlayDesc::new`] and the `with_*` setters; the runtime
/// clamps `volume` into 0..1 and `pitch` into the documented range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayDesc {
    /// The sound to play.
    pub sound: Sound,
    /// The bus to mix through, `0..`[`crate::BUS_COUNT`].
    pub bus: u32,
    /// Whether to loop forever.
    pub looping: bool,
    /// The voice's gain in 0..1.
    pub volume: f32,
    /// The voice's pitch factor.
    pub pitch: f32,
}

impl PlayDesc {
    /// A one-shot voice on the SFX bus at full volume and unity pitch.
    #[must_use]
    pub const fn new(sound: Sound) -> Self {
        Self {
            sound,
            bus: BUS_SFX,
            looping: false,
            volume: 1.0,
            pitch: 1.0,
        }
    }

    /// Sets the bus.
    #[must_use]
    pub const fn with_bus(mut self, bus: u32) -> Self {
        self.bus = bus;
        self
    }

    /// Sets looping.
    #[must_use]
    pub const fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Sets the gain; clamped by the runtime into 0..1.
    #[must_use]
    pub const fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// Sets the pitch factor; clamped by the runtime into the documented
    /// range.
    #[must_use]
    pub const fn with_pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }

    fn to_raw(self) -> AudioPlayDesc {
        AudioPlayDesc {
            sound: self.sound.to_raw(),
            bus: self.bus,
            looping: u32::from(self.looping),
            volume: self.volume,
            pitch: self.pitch,
        }
    }
}

/// The runtime's monotonic counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
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

impl From<AudioStats> for Stats {
    fn from(raw: AudioStats) -> Self {
        Self {
            voices_started: raw.voices_started,
            voices_finished: raw.voices_finished,
            blocks_mixed: raw.blocks_mixed,
            commands_applied: raw.commands_applied,
            commands_rejected: raw.commands_rejected,
            sounds_retired: raw.sounds_retired,
            voices_orphaned: raw.voices_orphaned,
        }
    }
}

/// An owning handle to one native playback runtime and mixer.
pub struct Runtime {
    handle: NonNull<AudioRuntime>,
    sample_rate: u32,
}

impl Runtime {
    /// Creates a runtime that mixes at `sample_rate` (8000..=192000). The
    /// runtime starts empty: no sounds, no voices, master and bus gains at
    /// 1.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::InvalidSampleRate`] for a rate outside the supported
    /// range, [`RuntimeError::OutOfMemory`] when the allocation failed.
    ///
    /// # Panics
    ///
    /// If the module reports success and returns null, which would be a
    /// module bug.
    pub fn new(sample_rate: u32) -> Result<Self, RuntimeError> {
        let mut handle: *mut AudioRuntime = core::ptr::null_mut();
        // SAFETY: `handle` is a live local valid for one write.
        let status = unsafe { mikan_audio_runtime_create(sample_rate, &mut handle) };
        if status == BAD_ARGUMENT {
            return Err(RuntimeError::InvalidSampleRate);
        }
        if status != OK {
            return Err(to_runtime_error(status));
        }
        let Some(handle) = NonNull::new(handle) else {
            panic!("mikan-audio-sys: the native runtime reported success but returned null");
        };
        Ok(Self {
            handle,
            sample_rate,
        })
    }

    /// The rate the mixer renders at; every voice's playback rate is
    /// derived from it.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Registers a sound by copying `samples` (interleaved, `channels`
    /// channels, at `sample_rate`) into runtime-owned memory and returns
    /// its handle. [`Runtime::remove_sound`] retires it again.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Unsupported`] when `channels` is neither 1 nor 2,
    /// [`RuntimeError::BadArgument`] when `samples` is empty, when its
    /// length is not a multiple of `channels`, or when `sample_rate` is
    /// out of range, [`RuntimeError::Capacity`] when all
    /// [`crate::MAX_SOUNDS`] slots are taken, [`RuntimeError::OutOfMemory`]
    /// when the copy's allocation failed.
    pub fn add_sound(
        &mut self,
        samples: &[f32],
        channels: u32,
        sample_rate: u32,
    ) -> Result<Sound, RuntimeError> {
        if channels == 0 {
            return Err(RuntimeError::BadArgument);
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(RuntimeError::BadArgument);
        }
        if samples.is_empty() {
            return Err(RuntimeError::BadArgument);
        }
        let frame_count = samples.len() / channels as usize;
        let frame_count = u32::try_from(frame_count).map_err(|_| RuntimeError::BadArgument)?;
        let mut raw = AudioSound {
            index: 0,
            generation: 0,
        };
        // SAFETY: the handle is live and borrowed exclusively; `samples`
        // is a live slice valid for `frame_count * channels` reads, which
        // is its whole length by construction; `raw` is a live local.
        let status = unsafe {
            mikan_audio_runtime_add_sound(
                self.handle.as_ptr(),
                samples.as_ptr(),
                frame_count,
                channels,
                sample_rate,
                &mut raw,
            )
        };
        if status != OK {
            return Err(to_runtime_error(status));
        }
        Sound::from_raw(raw).ok_or(RuntimeError::BadArgument)
    }

    /// Retires the sound `sound`: new [`Runtime::play`] calls are rejected
    /// immediately, its live voices are stopped by the render side, and
    /// its memory is freed by the next [`Runtime::collect`] (which
    /// [`Runtime::add_sound`] also runs). Idempotent while the handle names
    /// the sound, including between acknowledgment and collection.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] for a handle whose slot was recycled or
    /// that never named a sound, [`RuntimeError::Busy`] when the retirement
    /// could not be queued (queue full), in which case nothing changed.
    pub fn remove_sound(&mut self, sound: Sound) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status =
            unsafe { mikan_audio_runtime_remove_sound(self.handle.as_ptr(), sound.to_raw()) };
        status_to_unit(status)
    }

    /// Frees the buffers of sounds whose retirement the render side
    /// acknowledged and returns their slots to the pool.
    pub fn collect(&mut self) {
        // SAFETY: the handle is live and borrowed exclusively.
        unsafe { mikan_audio_runtime_collect(self.handle.as_ptr()) };
    }

    /// Whether the sound is registered and not retired. A stale handle —
    /// one whose slot was recycled — is simply not live.
    #[must_use]
    pub fn sound_live(&self, sound: Sound) -> bool {
        let mut live = 0_u32;
        // SAFETY: the handle is live; the call only reads atomics and
        // writes the live local.
        let status = unsafe {
            mikan_audio_runtime_sound_live(self.handle.as_ptr(), sound.to_raw(), &mut live)
        };
        debug_assert_eq!(status, OK, "a live handle cannot be rejected");
        live == 1
    }

    /// Allocates a voice for `desc` and hands it to the render side. The
    /// voice reports [`VoiceState::Playing`] immediately and begins mixing
    /// at the next render block; each call allocates a fresh voice, so
    /// asking twice starts two — that is how simultaneous voices are made.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] when the sound is unknown or retired,
    /// [`RuntimeError::BadArgument`] for a bus outside `0..`[`crate::BUS_COUNT`]
    /// or a non-finite volume or pitch, [`RuntimeError::Capacity`] when
    /// all [`crate::MAX_VOICES`] voices are taken, [`RuntimeError::Busy`]
    /// when the command queue is full (retry next block).
    ///
    /// # Panics
    ///
    /// If the module reports success and mints an invalid handle, which
    /// would be a module bug.
    pub fn play(&mut self, desc: PlayDesc) -> Result<Voice, RuntimeError> {
        let mut raw = AudioVoice {
            index: 0,
            generation: 0,
        };
        let raw_desc = desc.to_raw();
        // SAFETY: the handle is live and borrowed exclusively; the desc is
        // a live local; the out pointer is a live local.
        let status = unsafe { mikan_audio_runtime_play(self.handle.as_ptr(), &raw_desc, &mut raw) };
        if status != OK {
            return Err(to_runtime_error(status));
        }
        assert!(
            raw.generation != 0,
            "mikan-audio-sys: the native runtime minted a voice with generation zero"
        );
        Ok(Voice {
            index: raw.index,
            generation: raw.generation,
        })
    }

    /// Pauses a live voice; the position is preserved. Pausing a paused
    /// voice is fine.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] for a stale handle or one whose voice is
    /// finished, [`RuntimeError::Busy`] when the command queue is full.
    pub fn pause(&mut self, voice: Voice) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe { mikan_audio_runtime_pause(self.handle.as_ptr(), voice.to_raw()) };
        status_to_unit(status)
    }

    /// Resumes a paused voice; playing voices are left alone.
    ///
    /// # Errors
    ///
    /// As [`Runtime::pause`].
    pub fn resume(&mut self, voice: Voice) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe { mikan_audio_runtime_resume(self.handle.as_ptr(), voice.to_raw()) };
        status_to_unit(status)
    }

    /// Stops a live voice: the position resets to zero, the state becomes
    /// [`VoiceState::Finished`], and the slot is reclaimed by a later
    /// [`Runtime::play`]. Stopping an already-finished voice is
    /// [`RuntimeError::NotFound`]: there is nothing live to stop.
    ///
    /// # Errors
    ///
    /// As [`Runtime::pause`].
    pub fn stop(&mut self, voice: Voice) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe { mikan_audio_runtime_stop(self.handle.as_ptr(), voice.to_raw()) };
        status_to_unit(status)
    }

    /// Sets a live or finished voice's gain (clamped into 0..1). The
    /// change applies from the next render block.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] for a stale handle,
    /// [`RuntimeError::BadArgument`] for a non-finite volume or a
    /// generation-zero handle, [`RuntimeError::Busy`] when the queue is
    /// full.
    pub fn set_voice_volume(&mut self, voice: Voice, volume: f32) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe {
            mikan_audio_runtime_set_voice_volume(self.handle.as_ptr(), voice.to_raw(), volume)
        };
        status_to_unit(status)
    }

    /// Sets a live or finished voice's pitch factor (clamped into
    /// [`crate::MIN_PITCH`]..[`crate::MAX_PITCH`]). The change applies from the next
    /// render block.
    ///
    /// # Errors
    ///
    /// As [`Runtime::set_voice_volume`].
    pub fn set_voice_pitch(&mut self, voice: Voice, pitch: f32) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe {
            mikan_audio_runtime_set_voice_pitch(self.handle.as_ptr(), voice.to_raw(), pitch)
        };
        status_to_unit(status)
    }

    /// Sets one bus's gain (clamped into 0..1); the render side applies it
    /// at the next block without a queued command.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] for a bus outside `0..`[`crate::BUS_COUNT`]
    /// or a non-finite volume.
    pub fn set_bus_volume(&mut self, bus: u32, volume: f32) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status =
            unsafe { mikan_audio_runtime_set_bus_volume(self.handle.as_ptr(), bus, volume) };
        status_to_unit(status)
    }

    /// Sets the master gain (clamped into 0..1); the render side applies
    /// it at the next block without a queued command.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BadArgument`] for a non-finite volume.
    pub fn set_master_volume(&mut self, volume: f32) -> Result<(), RuntimeError> {
        // SAFETY: the handle is live and borrowed exclusively.
        let status = unsafe { mikan_audio_runtime_set_master_volume(self.handle.as_ptr(), volume) };
        status_to_unit(status)
    }

    /// Reads a voice's state. A stale handle reports
    /// [`VoiceState::Finished`] with a zeroed payload: the voice it named
    /// is over, and the payload deliberately does not borrow its
    /// successor's. A forged handle (out-of-range index, zero generation,
    /// or a generation from the future) is [`RuntimeError::NotFound`].
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotFound`] as described above.
    pub fn voice_state(&self, voice: Voice) -> Result<VoiceStatus, RuntimeError> {
        let mut raw = AudioVoiceState {
            state: 0,
            sound: AudioSound {
                index: 0,
                generation: 0,
            },
            looping: 0,
            volume: 0.0,
            pitch: 0.0,
            position_frames: 0,
        };
        // SAFETY: the handle is live; the call only reads atomics and
        // writes the live local.
        let status = unsafe {
            mikan_audio_runtime_voice_state(self.handle.as_ptr(), voice.to_raw(), &mut raw)
        };
        if status != OK {
            return Err(to_runtime_error(status));
        }
        let state = match raw.state {
            VOICE_PLAYING => VoiceState::Playing,
            VOICE_PAUSED => VoiceState::Paused,
            VOICE_FINISHED => VoiceState::Finished,
            other => {
                debug_assert!(false, "the module reported state {other}");
                VoiceState::Finished
            }
        };
        Ok(VoiceStatus {
            state,
            sound: Sound::from_raw(raw.sound),
            looping: raw.looping == 1,
            volume: raw.volume,
            pitch: raw.pitch,
            position_frames: raw.position_frames,
        })
    }

    /// Copies the runtime's counters.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let mut raw = AudioStats::default();
        // SAFETY: the handle is live; the call only reads atomics and
        // writes the live local.
        unsafe { mikan_audio_runtime_stats_into(self.handle.as_ptr(), &mut raw) };
        Stats::from(raw)
    }

    pub(crate) fn as_ptr(&self) -> *mut AudioRuntime {
        self.handle.as_ptr()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from `create` and is dropped exactly
        // once, here; no method can use it afterwards. The caller-side
        // shutdown order (backend first, per the header) is enforced by
        // `crate::system::System`'s field order, not by this type.
        unsafe { mikan_audio_runtime_destroy(self.handle.as_ptr()) };
    }
}

impl core::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Runtime")
            .field("sample_rate", &self.sample_rate)
            .field("stats", &self.stats())
            .finish()
    }
}
