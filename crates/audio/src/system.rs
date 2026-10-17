//! The unsafe-free audio system: device or offline rendering, sound
//! loading, voice control, and gain management.
//!
//! Everything with a pointer lives in `mikan-audio-sys`; this module owns
//! the ergonomics and the error mapping.

use std::path::Path;

use mikan_assets::{SoundData, SoundError};
use mikan_audio_sys as sys;
use thiserror::Error;

use crate::sound::{PlaySettings, Sound};
use crate::voice::{Voice, VoiceStatus, map_state};

/// Frames per render block when the caller passes zero, matching the native
/// module's documented default.
pub const DEFAULT_BLOCK_FRAMES: u32 = 1024;

/// Why an audio operation failed.
///
/// [`AudioError::Asset`] is the only variant that can come from outside the
/// audio module; the rest are the native module's codes, translated. See
/// [`AudioError::NoDevice`] for the one every caller should handle.
#[derive(Debug, Error)]
pub enum AudioError {
    /// No output device could be opened. The honest fallback is
    /// [`AudioSystem::new_offline`].
    #[error("no audio output device is available")]
    NoDevice,
    /// The device was opened but failed to start or stop.
    #[error("the audio device failed")]
    DeviceError,
    /// The requested sample rate is outside `8000..=192000`.
    #[error("the sample rate must be in 8000..=192000")]
    InvalidSampleRate,
    /// The native module rejected an argument (a non-finite gain, an
    /// out-of-range bus, or a similar programming mistake).
    #[error("the audio module rejected an argument")]
    InvalidArgument,
    /// The data is outside the mixer's small format set.
    #[error("the audio module does not support this input")]
    Unsupported,
    /// A pool is exhausted: all voices are taken, or all sound slots are.
    #[error("the audio module is out of capacity")]
    Capacity,
    /// The handle names nothing: its slot was already recycled, or the sound
    /// was already unloaded.
    #[error("the audio handle no longer names anything")]
    NotFound,
    /// The command queue is full. Retry after the next
    /// [`AudioSystem::update`].
    #[error("the audio command queue is full")]
    Busy,
    /// An allocation failed.
    #[error("the audio module is out of memory")]
    OutOfMemory,
    /// [`AudioSystem::drain_samples`] was called on a device backend, where
    /// there is no ring to drain.
    #[error("output samples can only be drained on the offline backend")]
    OfflineOnly,
    /// A sound could not be read or decoded.
    #[error(transparent)]
    Asset(#[from] SoundError),
}

/// Turns a native layer's error into the safe one.
fn convert(error: impl Into<AudioError>) -> AudioError {
    error.into()
}

impl From<sys::AudioError> for AudioError {
    fn from(error: sys::AudioError) -> Self {
        match error {
            sys::AudioError::NoDevice => Self::NoDevice,
            sys::AudioError::DeviceError => Self::DeviceError,
            sys::AudioError::InvalidSampleRate => Self::InvalidSampleRate,
            sys::AudioError::BadArgument => Self::InvalidArgument,
            sys::AudioError::Unsupported => Self::Unsupported,
            sys::AudioError::Capacity => Self::Capacity,
            sys::AudioError::NotFound => Self::NotFound,
            sys::AudioError::Busy => Self::Busy,
            sys::AudioError::OutOfMemory => Self::OutOfMemory,
        }
    }
}

impl From<sys::RuntimeError> for AudioError {
    fn from(error: sys::RuntimeError) -> Self {
        match error {
            sys::RuntimeError::InvalidSampleRate => Self::InvalidSampleRate,
            sys::RuntimeError::BadArgument => Self::InvalidArgument,
            sys::RuntimeError::Unsupported => Self::Unsupported,
            sys::RuntimeError::Capacity => Self::Capacity,
            sys::RuntimeError::NotFound => Self::NotFound,
            sys::RuntimeError::Busy => Self::Busy,
            sys::RuntimeError::OutOfMemory => Self::OutOfMemory,
        }
    }
}

/// The audio system: one mixer, one device (or one offline ring), and the
/// handles for everything inside them.
///
/// # Lifecycle
///
/// Construct one per game, keep it somewhere the game loop can reach, call
/// [`AudioSystem::update`] once per frame (it is a no-op on the device
/// path, where the mixer runs on the audio thread), and drop it when the
/// game exits. Dropping stops and joins the audio thread before freeing the
/// mixer, in that order, so no callback can touch freed memory.
///
/// The system is deliberately neither `Send` nor `Sync`: sounds and voices
/// are controlled from one thread, and the native module's thread safety is
/// exactly this contract.
///
/// # Failure model
///
/// Operations that cannot do what was asked return an [`AudioError`];
/// operations that can degrade (a non-finite gain, an out-of-range bus) are
/// rejected the same way, while values that merely exceed a range are
/// clamped. Nothing panics on bad input, and nothing here can abort the
/// audio thread: the render callback never allocates, never locks, and
/// never reports errors.
pub struct AudioSystem {
    system: sys::System,
    block_frames: u32,
}

impl AudioSystem {
    /// Opens the default output device and starts it.
    ///
    /// `sample_rate` is the rate the mixer runs at; pass `0` for the
    /// module's default (48 kHz). `block_frames` is the device period in
    /// frames; pass `0` for [`DEFAULT_BLOCK_FRAMES`].
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NoDevice`] when no output device exists,
    /// [`AudioError::DeviceError`] when one exists but will not open,
    /// [`AudioError::InvalidSampleRate`] for a rate outside
    /// `8000..=192000`, and [`AudioError::OutOfMemory`] when the mixer
    /// cannot be allocated.
    pub fn new_device(sample_rate: u32, block_frames: u32) -> Result<Self, AudioError> {
        let mut system = sys::System::new_device(sample_rate, block_frames).map_err(convert)?;
        system.start().map_err(convert)?;
        Ok(Self {
            system,
            block_frames: resolved_block_frames(block_frames),
        })
    }

    /// Creates the deterministic offline backend: no device is opened, and
    /// [`AudioSystem::update`] renders one `block_frames` block into a ring
    /// that [`AudioSystem::drain_samples`] empties.
    ///
    /// This is the path tests and headless environments use; it exercises
    /// the same mixer code a device callback runs.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::InvalidSampleRate`] for a rate outside
    /// `8000..=192000` and [`AudioError::OutOfMemory`] when the mixer or
    /// the ring cannot be allocated.
    pub fn new_offline(sample_rate: u32, block_frames: u32) -> Result<Self, AudioError> {
        let system = sys::System::new_offline(sample_rate, block_frames).map_err(convert)?;
        Ok(Self {
            system,
            block_frames: resolved_block_frames(block_frames),
        })
    }

    /// Whether this system renders offline instead of on the audio thread.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.system.info().kind == sys::BackendKind::Offline
    }

    /// The rate the mixer renders at.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.system.runtime().sample_rate()
    }

    /// The name of the backend in use ("ALSA", "CoreAudio", "offline", ...).
    #[must_use]
    pub fn backend_name(&self) -> String {
        self.system.info().backend_name
    }

    /// The opened device's name, or "offline".
    #[must_use]
    pub fn device_name(&self) -> String {
        self.system.info().device_name
    }

    /// The mixer's counters (voices started, finished, sounds loaded,
    /// overruns, ...). Cheap to read; useful for an audio debug overlay.
    #[must_use]
    pub fn stats(&self) -> sys::Stats {
        self.system.runtime().stats()
    }

    /// Copies `data` into the mixer and returns its handle.
    ///
    /// Loading costs one decode and one copy; playing the returned handle
    /// any number of times costs neither again.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::Capacity`] when all 256 sound slots are taken,
    /// [`AudioError::Unsupported`] when the layout is not mono or stereo,
    /// [`AudioError::InvalidArgument`] when the data is empty or its rate is
    /// out of range, and [`AudioError::OutOfMemory`] when the copy cannot be
    /// allocated. Mixer slots whose sounds were unloaded earlier in this
    /// session are reclaimed here.
    pub fn load_sound(&mut self, data: &SoundData) -> Result<Sound, AudioError> {
        let raw = self
            .system
            .runtime_mut()
            .add_sound(data.samples(), data.channels(), data.sample_rate())
            .map_err(convert)?;
        Ok(Sound::from_raw(raw))
    }

    /// Decodes a WAV file through the asset pipeline and loads it, in one
    /// step. For files already decoded as [`SoundData`], use
    /// [`AudioSystem::load_sound`].
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::Asset`] when the file cannot be read or is
    /// outside the supported WAV subset, and the same errors as
    /// [`AudioSystem::load_sound`].
    pub fn load_wav(&mut self, path: impl AsRef<Path>) -> Result<Sound, AudioError> {
        let data = SoundData::load_wav(path)?;
        self.load_sound(&data)
    }

    /// Unloads a sound: new [`AudioSystem::play`] calls with this handle
    /// fail immediately, its live voices stop at the next render block, and
    /// its mixer slot is reclaimed once the render side has acknowledged
    /// the removal (the next [`AudioSystem::update`] on the offline path).
    /// Idempotent until the slot is reclaimed.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NotFound`] when the handle no longer names a
    /// loaded sound, and [`AudioError::Busy`] when the removal cannot be
    /// queued because the command queue is full (nothing changes; retry
    /// after [`AudioSystem::update`]).
    pub fn unload_sound(&mut self, sound: Sound) -> Result<(), AudioError> {
        self.system
            .runtime_mut()
            .remove_sound(sound.raw)
            .map_err(convert)?;
        self.system.runtime_mut().collect();
        Ok(())
    }

    /// Whether the handle names a loaded, not-yet-unloaded sound.
    #[must_use]
    pub fn sound_loaded(&self, sound: Sound) -> bool {
        self.system.runtime().sound_live(sound.raw)
    }

    /// Starts one voice of `sound`. The system's voice pool holds 64
    /// simultaneous voices; starting more fails cleanly with
    /// [`AudioError::Capacity`] until a voice finishes or is stopped.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NotFound`] when the sound was unloaded,
    /// [`AudioError::Capacity`] when every voice is busy,
    /// [`AudioError::InvalidArgument`] when a bus, volume, or pitch is
    /// invalid (non-finite), and [`AudioError::Busy`] when the command
    /// queue is full.
    pub fn play(&mut self, sound: Sound, settings: PlaySettings) -> Result<Voice, AudioError> {
        let desc = sys::PlayDesc::new(sound.raw)
            .with_bus(settings.bus.to_raw())
            .with_looping(settings.looping)
            .with_volume(settings.volume)
            .with_pitch(settings.pitch);
        let raw = self.system.runtime_mut().play(desc).map_err(convert)?;
        Ok(Voice::from_raw(raw))
    }

    /// Pauses a voice at its current position. Applies at the next render
    /// block; pausing a paused voice is a no-op that still succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NotFound`] when the voice already finished,
    /// and [`AudioError::Busy`] when the command queue is full.
    pub fn pause(&mut self, voice: Voice) -> Result<(), AudioError> {
        self.system.runtime_mut().pause(voice.raw).map_err(convert)
    }

    /// Resumes a paused voice from where it stopped.
    ///
    /// # Errors
    ///
    /// The same as [`AudioSystem::pause`].
    pub fn resume(&mut self, voice: Voice) -> Result<(), AudioError> {
        self.system.runtime_mut().resume(voice.raw).map_err(convert)
    }

    /// Stops a voice. Its position resets to the beginning of the sound and
    /// its state becomes [`crate::PlaybackState::Finished`]; the slot is
    /// reusable for a new voice. Stopping an already-finished voice is
    /// [`AudioError::NotFound`].
    ///
    /// # Errors
    ///
    /// The same as [`AudioSystem::pause`].
    pub fn stop(&mut self, voice: Voice) -> Result<(), AudioError> {
        self.system.runtime_mut().stop(voice.raw).map_err(convert)
    }

    /// Sets a voice's gain; clamped into `0..=1`. Applies at the next
    /// render block. Works on a playing, paused, or finished-but-not-yet-
    /// recycled voice.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::InvalidArgument`] for a non-finite value,
    /// [`AudioError::NotFound`] for a handle whose slot was recycled, and
    /// [`AudioError::Busy`] when the command queue is full.
    pub fn set_voice_volume(&mut self, voice: Voice, volume: f32) -> Result<(), AudioError> {
        self.system
            .runtime_mut()
            .set_voice_volume(voice.raw, volume)
            .map_err(convert)
    }

    /// Sets a voice's pitch factor; clamped into `0.25..=4.0`. Applies at
    /// the next render block.
    ///
    /// # Errors
    ///
    /// The same as [`AudioSystem::set_voice_volume`].
    pub fn set_voice_pitch(&mut self, voice: Voice, pitch: f32) -> Result<(), AudioError> {
        self.system
            .runtime_mut()
            .set_voice_pitch(voice.raw, pitch)
            .map_err(convert)
    }

    /// Sets one bus's gain; clamped into `0..=1`. Applies from the next
    /// render block without queueing a command.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::InvalidArgument`] for a non-finite value.
    pub fn set_bus_volume(&mut self, bus: crate::Bus, volume: f32) -> Result<(), AudioError> {
        self.system
            .runtime_mut()
            .set_bus_volume(bus.to_raw(), volume)
            .map_err(convert)
    }

    /// Sets the master gain; clamped into `0..=1`. Applies from the next
    /// render block without queueing a command.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::InvalidArgument`] for a non-finite value.
    pub fn set_master_volume(&mut self, volume: f32) -> Result<(), AudioError> {
        self.system
            .runtime_mut()
            .set_master_volume(volume)
            .map_err(convert)
    }

    /// Reads a voice's state without blocking the audio thread.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NotFound`] only for a handle that could never
    /// have been minted; a stale-but-real handle reports
    /// [`crate::PlaybackState::Finished`] with a zeroed payload, as
    /// documented on [`Voice`].
    pub fn voice_status(&self, voice: Voice) -> Result<VoiceStatus, AudioError> {
        let status = self
            .system
            .runtime()
            .voice_state(voice.raw)
            .map_err(convert)?;
        Ok(VoiceStatus {
            state: map_state(status.state),
            sound: status.sound.map(Sound::from_raw),
            looping: status.looping,
            volume: status.volume,
            pitch: status.pitch,
            position_frames: status.position_frames,
        })
    }

    /// Advances the audio system by one frame of the game loop.
    ///
    /// On the device path this is a no-op — the mixer already runs on the
    /// audio thread — and it exists so a game has one call that is correct
    /// on both paths. On the offline path it renders exactly one
    /// `block_frames` block into the ring, reclaims unloaded sounds, and
    /// fails (with an honest [`AudioError`]) only when the mixer itself
    /// cannot render.
    ///
    /// # Errors
    ///
    /// Returns any error the render or the reclamation produced; on the
    /// device path, none.
    pub fn update(&mut self) -> Result<(), AudioError> {
        if self.is_offline() {
            self.system.pump(self.block_frames).map_err(convert)?;
            self.system.runtime_mut().collect();
        }
        Ok(())
    }

    /// Takes rendered stereo samples off the offline ring, as interleaved
    /// `i16` in the device's output format. Returns the number of frames
    /// written; zero means the ring was empty.
    ///
    /// Offline only: the device path has no ring to drain, because the
    /// samples went to the sound card.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::OfflineOnly`] on the device path, and
    /// [`AudioError::InvalidArgument`] when the buffer is empty.
    pub fn drain_samples(&mut self, out: &mut [i16]) -> Result<usize, AudioError> {
        if !self.is_offline() {
            return Err(AudioError::OfflineOnly);
        }
        self.system.read_s16(out).map_err(convert)
    }

    /// How many offline blocks were dropped because the ring was full.
    /// Always zero on the device path.
    #[must_use]
    pub fn overruns(&self) -> u32 {
        self.system.overruns()
    }
}

/// The block size the mixer will actually use.
fn resolved_block_frames(block_frames: u32) -> u32 {
    if block_frames == 0 {
        DEFAULT_BLOCK_FRAMES
    } else {
        block_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A short ramp: 16 mono frames ending exactly where they started.
    fn ramp() -> SoundData {
        let samples = (0..16).map(|i| (i as f32 - 8.0) / 16.0).collect();
        SoundData::from_samples(samples, 1, 48_000).expect("valid ramp")
    }

    #[test]
    fn an_offline_system_plays_sounds_and_drains_samples() {
        let mut audio = AudioSystem::new_offline(48_000, 64).expect("offline system");
        assert!(audio.is_offline());
        assert_eq!(audio.sample_rate(), 48_000);
        assert_eq!(audio.backend_name(), "offline");
        assert_eq!(audio.device_name(), "offline");

        let sound = audio.load_sound(&ramp()).expect("load");
        assert!(audio.sound_loaded(sound));
        let voice = audio
            .play(sound, PlaySettings::new())
            .expect("play a free voice");
        assert_eq!(
            audio.voice_status(voice).expect("status").state,
            crate::PlaybackState::Playing,
            "queued voices report playing before the first block"
        );

        audio.update().expect("render a block");
        let mut out = vec![0_i16; 64 * 2];
        assert_eq!(audio.drain_samples(&mut out).expect("drain"), 64);
        assert!(
            out.iter().any(|&sample| sample != 0),
            "the ramp produced audible samples"
        );

        // The 16-frame ramp is finished after one 64-frame block.
        let status = audio.voice_status(voice).expect("status");
        assert_eq!(status.state, crate::PlaybackState::Finished);
        assert_eq!(status.sound, Some(sound));
        assert_eq!(status.position_frames, 16);

        // The ring is empty until the next update.
        assert_eq!(audio.drain_samples(&mut out).expect("drain"), 0);

        // A silence block renders silence, not an error.
        audio.update().expect("another block");
        assert_eq!(audio.drain_samples(&mut out).expect("drain"), 64);
        assert!(out.iter().all(|&sample| sample == 0));
    }

    #[test]
    fn two_different_sounds_sum_and_clip_predictably() {
        let mut audio = AudioSystem::new_offline(48_000, 8).expect("offline system");
        // DC sources make the expected output exact: 8 frames is one block,
        // so nothing wraps or interpolates.
        let quiet = audio
            .load_sound(&SoundData::from_samples(vec![0.25; 8], 1, 48_000).expect("valid"))
            .expect("load");
        let loud = audio
            .load_sound(&SoundData::from_samples(vec![0.5; 8], 1, 48_000).expect("valid"))
            .expect("load");

        // Both at half gain on the same bus: 0.25*0.5 + 0.5*0.5 = 0.375, and
        // to_s16(0.375) = floor(0.375*32767 + 0.5) = 12288.
        audio
            .play(quiet, PlaySettings::new().with_volume(0.5))
            .expect("play");
        audio
            .play(loud, PlaySettings::new().with_volume(0.5))
            .expect("play");
        audio.update().expect("render");
        let mut out = vec![0_i16; 16];
        assert_eq!(audio.drain_samples(&mut out).expect("drain"), 8);
        assert!(
            out.iter().all(|&sample| sample == 12_288),
            "two voices must sum: {out:?}"
        );

        // Both at full gain: 0.25 + 0.5 = 0.75 -> floor(0.75*32767 + 0.5) =
        // 24575. Then four copies of the loud sound: 4*0.5 = 2.0, which must
        // clip to exactly 1.0 (32767), not wrap.
        audio
            .play(quiet, PlaySettings::new())
            .expect("play at full gain");
        audio
            .play(loud, PlaySettings::new())
            .expect("play at full gain");
        for _ in 0..3 {
            audio
                .play(loud, PlaySettings::new())
                .expect("play at full gain");
        }
        audio.update().expect("render");
        assert_eq!(audio.drain_samples(&mut out).expect("drain"), 8);
        assert!(
            out.iter().all(|&sample| sample == 32_767),
            "saturated sums must clip, not wrap: {out:?}"
        );
        assert_eq!(audio.overruns(), 0);
    }

    #[test]
    fn loading_more_than_the_pool_holds_reports_capacity() {
        let mut audio = AudioSystem::new_offline(48_000, 16).expect("offline system");
        let sound = audio
            .load_sound(&SoundData::from_samples(vec![0.5; 4], 1, 48_000).expect("valid"))
            .expect("load");
        let mut voices = Vec::new();
        for _ in 0..64 {
            voices.push(
                audio
                    .play(sound, PlaySettings::new())
                    .expect("a free voice"),
            );
        }
        assert!(matches!(
            audio.play(sound, PlaySettings::new()),
            Err(AudioError::Capacity)
        ));

        // Draining a block lets the four-frame sounds finish and frees
        // every slot again.
        audio.update().expect("render");
        let mut out = vec![0_i16; 32];
        audio.drain_samples(&mut out).expect("drain");
        audio
            .play(sound, PlaySettings::new())
            .expect("the slots are reusable");
    }

    #[test]
    fn unloading_stops_and_frees() {
        let mut audio = AudioSystem::new_offline(48_000, 32).expect("offline system");
        let sound = audio.load_sound(&ramp()).expect("load");
        let voice = audio.play(sound, PlaySettings::new()).expect("play");
        audio.unload_sound(sound).expect("unload");
        assert!(!audio.sound_loaded(sound));
        assert!(matches!(
            audio.play(sound, PlaySettings::new()),
            Err(AudioError::NotFound)
        ));

        // The next update stops the orphaned voice and reclaims the slot.
        audio.update().expect("render");
        let status = audio.voice_status(voice).expect("status");
        assert_eq!(status.state, crate::PlaybackState::Finished);

        // The slot is reusable, and the old handle names nothing.
        let replacement = audio.load_sound(&ramp()).expect("the slot is reusable");
        assert!(!audio.sound_loaded(sound));
        assert!(audio.sound_loaded(replacement));
        assert!(matches!(
            audio.play(sound, PlaySettings::new()),
            Err(AudioError::NotFound)
        ));
    }

    #[test]
    fn controls_reach_the_mixer_state() {
        let mut audio = AudioSystem::new_offline(48_000, 32).expect("offline system");
        let sound = audio.load_sound(&ramp()).expect("load");
        let voice = audio
            .play(
                sound,
                PlaySettings::new()
                    .with_bus(crate::Bus::Music)
                    .with_looping(true)
                    .with_volume(2.0)
                    .with_pitch(0.1),
            )
            .expect("play");
        audio.update().expect("render");
        let status = audio.voice_status(voice).expect("status");
        assert_eq!(status.volume, 1.0, "volume clamps into 0..=1");
        assert_eq!(status.pitch, 0.25, "pitch clamps into 0.25..=4.0");
        assert!(status.looping);

        audio.pause(voice).expect("pause");
        audio.update().expect("render");
        assert_eq!(
            audio.voice_status(voice).expect("status").state,
            crate::PlaybackState::Paused
        );
        audio.resume(voice).expect("resume");
        audio.set_voice_volume(voice, 0.5).expect("volume");
        audio.set_voice_pitch(voice, 2.0).expect("pitch");
        audio.set_bus_volume(crate::Bus::Music, 0.25).expect("bus");
        audio.set_master_volume(0.5).expect("master");
        audio.update().expect("render");
        let status = audio.voice_status(voice).expect("status");
        assert_eq!(status.state, crate::PlaybackState::Playing);
        assert_eq!(status.volume, 0.5);
        assert_eq!(status.pitch, 2.0);

        audio.stop(voice).expect("stop");
        audio.update().expect("render");
        let status = audio.voice_status(voice).expect("status");
        assert_eq!(status.state, crate::PlaybackState::Finished);
        assert_eq!(status.position_frames, 0, "a stop rewinds");
    }

    #[test]
    fn invalid_gains_are_rejected_and_bad_rates_reported() {
        let mut audio = AudioSystem::new_offline(48_000, 32).expect("offline system");
        let sound = audio.load_sound(&ramp()).expect("load");
        assert!(matches!(
            audio.set_master_volume(f32::NAN),
            Err(AudioError::InvalidArgument)
        ));
        assert!(matches!(
            audio.set_bus_volume(crate::Bus::Sfx, f32::INFINITY),
            Err(AudioError::InvalidArgument)
        ));
        assert!(matches!(
            audio.play(sound, PlaySettings::new().with_volume(f32::NAN)),
            Err(AudioError::InvalidArgument)
        ));
        assert!(matches!(
            AudioSystem::new_offline(100, 32),
            Err(AudioError::InvalidSampleRate)
        ));
        assert!(matches!(
            AudioSystem::new_offline(192_001, 32),
            Err(AudioError::InvalidSampleRate)
        ));
    }

    #[test]
    fn the_default_block_size_matches_the_native_default() {
        let mut audio = AudioSystem::new_offline(48_000, 0).expect("offline system");
        audio.update().expect("render");
        let mut out = vec![0_i16; (DEFAULT_BLOCK_FRAMES * 2) as usize];
        assert_eq!(
            audio.drain_samples(&mut out).expect("drain"),
            DEFAULT_BLOCK_FRAMES as usize,
            "passing zero must render the documented default block"
        );
    }

    #[test]
    fn missing_files_reach_the_caller_as_asset_errors() {
        let mut audio = AudioSystem::new_offline(48_000, 32).expect("offline system");
        let path = std::env::temp_dir().join("mikan-audio-missing-file.wav");
        match audio.load_wav(&path) {
            Err(AudioError::Asset(SoundError::Io {
                path: reported,
                source,
            })) => {
                assert_eq!(reported, path);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected an asset error, got {other:?}"),
        }
    }

    #[test]
    fn opening_and_draining_the_device_path_agree_with_the_hardware() {
        // This machine may have no device; both outcomes are correct, and
        // both must agree with the backend that was selected.
        match AudioSystem::new_device(48_000, 256) {
            Ok(mut audio) => {
                assert!(!audio.is_offline());
                assert_eq!(audio.sample_rate(), 48_000);
                let mut out = vec![0_i16; 16];
                assert!(matches!(
                    audio.drain_samples(&mut out),
                    Err(AudioError::OfflineOnly)
                ));
                assert_eq!(audio.overruns(), 0);
            }
            Err(AudioError::NoDevice) => {}
            Err(other) => panic!("unexpected device failure: {other}"),
        }
    }
}
