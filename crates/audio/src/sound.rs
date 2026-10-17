//! Sound handles, buses, and the settings a voice starts with.

use mikan_audio_sys as sys;

/// Which of the mixer's two gains a voice mixes through.
///
/// The buses exist so a game can fade all effects or all music with one
/// call ([`AudioSystem::set_bus_volume`](crate::AudioSystem::set_bus_volume))
/// without walking its voices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Bus {
    /// Short, gameplay-triggered sounds (the default).
    #[default]
    Sfx,
    /// Long-running background audio.
    Music,
}

impl Bus {
    /// The raw `MIKAN_AUDIO_BUS_*` value.
    #[must_use]
    pub(crate) const fn to_raw(self) -> u32 {
        match self {
            Self::Sfx => sys::BUS_SFX,
            Self::Music => sys::BUS_MUSIC,
        }
    }
}

/// A loaded sound owned by the [`AudioSystem`](crate::AudioSystem) that
/// minted it.
///
/// The handle is small and copyable: storing it in a scene, an entity
/// component, or a resource is the intended use. The samples live in the
/// mixer, not here, so passing a `Sound` around never copies audio. The
/// handle is generation-checked, so a handle to an unloaded sound can never
/// name a later sound that reuses its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sound {
    pub(crate) raw: sys::Sound,
}

impl Sound {
    /// Wraps a handle the runtime minted.
    pub(crate) const fn from_raw(raw: sys::Sound) -> Self {
        Self { raw }
    }
}

/// How to start a voice.
///
/// The defaults are a one-shot effect on [`Bus::Sfx`] at full volume and
/// unity pitch. `volume` is clamped into `0..=1` and `pitch` into the
/// mixer's documented `0.25..=4.0` range by the native module, so
/// out-of-range values are safe (they just clamp) while non-finite ones are
/// rejected with [`AudioError::InvalidArgument`](crate::AudioError::InvalidArgument).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaySettings {
    /// The bus this voice mixes through.
    pub bus: Bus,
    /// Whether the voice loops until stopped.
    pub looping: bool,
    /// The voice's gain in `0..=1` (clamped).
    pub volume: f32,
    /// The voice's pitch factor (clamped).
    pub pitch: f32,
}

impl PlaySettings {
    /// A one-shot voice on the SFX bus at full volume and unity pitch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bus: Bus::Sfx,
            looping: false,
            volume: 1.0,
            pitch: 1.0,
        }
    }

    /// Sets the bus.
    #[must_use]
    pub const fn with_bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self
    }

    /// Sets looping.
    #[must_use]
    pub const fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Sets the gain; clamped into `0..=1`.
    #[must_use]
    pub const fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// Sets the pitch factor; clamped into the documented range.
    #[must_use]
    pub const fn with_pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }
}

impl Default for PlaySettings {
    fn default() -> Self {
        Self::new()
    }
}
