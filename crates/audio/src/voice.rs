//! Voice handles and the state a caller can observe.

use mikan_audio_sys as sys;

use crate::sound::Sound;

/// One playing (or finished) voice, as minted by
/// [`AudioSystem::play`](crate::AudioSystem::play).
///
/// The handle is small and copyable, and it is generation-checked: after its
/// voice finishes, it keeps reporting [`PlaybackState::Finished`] even if the
/// mixer gives the slot to a new voice, so an old handle can never be
/// confused for a live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Voice {
    pub(crate) raw: sys::Voice,
}

impl Voice {
    /// Wraps a handle the runtime minted.
    pub(crate) const fn from_raw(raw: sys::Voice) -> Self {
        Self { raw }
    }
}

/// What a voice is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    /// Mixing (or queued to start at the next render block).
    Playing,
    /// Paused; the position is preserved.
    Paused,
    /// Terminal: completed, stopped, unloaded with its sound, or recycled
    /// (the voice this handle named is over).
    Finished,
}

/// A snapshot of a voice's visible state.
///
/// The snapshot is read from the mixer's published state and never blocks
/// the audio thread; a change queued but not yet applied (for example a
/// pause made between two render blocks) becomes visible after the next
/// [`AudioSystem::update`](crate::AudioSystem::update) or device block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceStatus {
    /// What the voice is doing.
    pub state: PlaybackState,
    /// The sound the voice plays, when it has one.
    pub sound: Option<Sound>,
    /// Whether the voice loops.
    pub looping: bool,
    /// The voice's current gain, `0..=1`.
    pub volume: f32,
    /// The voice's current pitch factor.
    pub pitch: f32,
    /// Frames consumed of the sound; a stop resets it to zero, a natural
    /// completion leaves it at the sound's frame count.
    pub position_frames: u64,
}

pub(crate) fn map_state(state: sys::VoiceState) -> PlaybackState {
    match state {
        sys::VoiceState::Playing => PlaybackState::Playing,
        sys::VoiceState::Paused => PlaybackState::Paused,
        sys::VoiceState::Finished => PlaybackState::Finished,
    }
}
