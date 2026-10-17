//! The audio system: sounds in, mixed stereo out.
//!
//! This crate is the safe face of Phase 9. It owns the engine-facing
//! decisions the rest of Mikan leans on:
//!
//! 1. **Sounds are loaded once, through the asset pipeline.** A
//!    [`SoundData`] (decoded by `mikan-assets` from a WAV file, or built
//!    from samples) is copied into the mixer by
//!    [`AudioSystem::load_sound`]; [`Sound`] is the resulting handle.
//!    Playing the same sound many times never decodes or copies it again.
//! 2. **Playback is voices, and voices are handles.** Each
//!    [`AudioSystem::play`] starts one independent voice of a loaded
//!    sound. Voices pause, resume, stop, loop, and play at a pitch; a
//!    finished voice's handle keeps reporting [`PlaybackState::Finished`],
//!    never whatever voice later reuses its slot.
//! 3. **Two buses and a master gain.** Every voice mixes through
//!    [`Bus::Sfx`] or [`Bus::Music`]; the three gains clamp into `0..=1`
//!    and apply from the next render block.
//! 4. **The render side is separate.** The mixer runs on the audio
//!    thread (or, offline, on [`AudioSystem::update`]); control calls
//!    queue commands and are safe to make while sound is playing.
//!
//! The native C backend and C++ runtime live in `mikan-audio-sys`; this
//! crate contains no `unsafe`. `docs/audio.md` is the reference for the
//! playback contract — what a stop does to the position, when volume
//! changes apply, what happens when the voice pool is full.
//!
//! # Hardware and the offline path
//!
//! [`AudioSystem::new_device`] opens the default output device. On a
//! machine without one it fails with [`AudioError::NoDevice`] rather than
//! pretending: [`AudioSystem::new_offline`] is the explicit deterministic
//! fallback, where [`AudioSystem::update`] renders one block into a ring
//! the application drains with [`AudioSystem::drain_samples`]. Tests and
//! the demo's audio lane use the offline path, so nothing here needs
//! audio hardware.
//!
//! # Example
//!
//! ```
//! use mikan_audio::{AudioSystem, PlaybackState, PlaySettings, SoundData};
//!
//! let mut audio = AudioSystem::new_offline(48_000, 256)?;
//! let samples = SoundData::from_samples(vec![0.25; 16], 1, 48_000)?;
//! let blip = audio.load_sound(&samples)?;
//! let voice = audio.play(blip, PlaySettings::new())?;
//!
//! audio.update()?;
//! assert_eq!(audio.voice_status(voice)?.state, PlaybackState::Finished);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod sound;
mod system;
mod voice;

pub use sound::{Bus, PlaySettings, Sound};
pub use system::{AudioError, AudioSystem, DEFAULT_BLOCK_FRAMES};
pub use voice::{PlaybackState, Voice, VoiceStatus};

/// Re-exported so callers of [`AudioSystem::load_sound`] can name the data
/// they load without depending on `mikan-assets` directly, and so asset
/// errors convert into [`AudioError`] in one step.
pub use mikan_assets::{SoundData, SoundError};

/// The mixer's counters, re-exported from the native module.
pub use mikan_audio_sys::Stats as AudioStats;
