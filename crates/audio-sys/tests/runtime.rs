//! Tests for the playback runtime's semantics, driven through the offline
//! system so rendering is deterministic and no audio hardware is needed.

use mikan_audio_sys::{
    AudioError, BUS_COUNT, BUS_MUSIC, DEFAULT_SAMPLE_RATE, MAX_VOICES, PlayDesc, RuntimeError,
    System, Voice, VoiceState,
};

/// A mono signal built from eighths: every value is exactly representable
/// in f32, which keeps the arithmetic checks unambiguous.
fn dyadic(len: usize) -> Vec<f32> {
    (0..len).map(|i| ((i % 8) as f32 - 3.5) / 4.0).collect()
}

#[test]
fn the_runtime_validates_its_sample_rate() {
    assert_eq!(
        System::new_offline(7_999, 32).unwrap_err(),
        AudioError::InvalidSampleRate
    );
    assert_eq!(
        System::new_offline(192_001, 32).unwrap_err(),
        AudioError::InvalidSampleRate
    );
    let system = System::new_offline(0, 32).expect("zero selects the default rate");
    assert_eq!(system.runtime().sample_rate(), DEFAULT_SAMPLE_RATE);
    let system = System::new_offline(8_000, 32).expect("the floor is accepted");
    assert_eq!(system.runtime().sample_rate(), 8_000);
}

#[test]
fn adding_a_sound_validates_its_input() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let runtime = system.runtime_mut();
    assert_eq!(
        runtime.add_sound(&[], 1, 48_000).unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime.add_sound(&[0.0; 3], 2, 48_000).unwrap_err(),
        RuntimeError::BadArgument,
        "three samples do not make whole stereo frames"
    );
    assert_eq!(
        runtime.add_sound(&[0.0; 6], 3, 48_000).unwrap_err(),
        RuntimeError::Unsupported,
        "only mono and stereo are supported"
    );
    assert_eq!(
        runtime.add_sound(&[0.0; 4], 0, 48_000).unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime.add_sound(&[0.0; 4], 1, 100).unwrap_err(),
        RuntimeError::BadArgument
    );
    let sound = runtime
        .add_sound(&[0.0; 4], 2, 44_100)
        .expect("stereo at another rate");
    assert!(runtime.sound_live(sound));
}

#[test]
fn play_validates_its_desc() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(64), 1, 48_000)
        .expect("sound");
    let runtime = system.runtime_mut();
    assert_eq!(
        runtime
            .play(PlayDesc::new(sound).with_bus(BUS_COUNT))
            .unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime
            .play(PlayDesc::new(sound).with_volume(f32::NAN))
            .unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime
            .play(PlayDesc::new(sound).with_pitch(0.0))
            .unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime
            .play(PlayDesc::new(sound).with_pitch(f32::INFINITY))
            .unwrap_err(),
        RuntimeError::BadArgument
    );
    assert!(
        runtime
            .play(PlayDesc::new(sound).with_bus(BUS_MUSIC))
            .is_ok(),
        "the music bus is valid"
    );
}

#[test]
fn a_retired_sound_cannot_be_played_and_a_recycled_handle_is_not_the_new_sound() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let first = system
        .runtime_mut()
        .add_sound(&dyadic(64), 1, 48_000)
        .expect("sound");
    system
        .runtime_mut()
        .remove_sound(first)
        .expect("retirement is accepted");
    assert!(!system.runtime().sound_live(first));
    assert_eq!(
        system.runtime_mut().play(PlayDesc::new(first)).unwrap_err(),
        RuntimeError::NotFound
    );

    // The replacement's `add` collects the retired slot, so the two
    // sounds share an index; the generations must keep them apart.
    let second = system
        .runtime_mut()
        .add_sound(&dyadic(64), 1, 48_000)
        .expect("the slot is reusable");
    assert!(system.runtime().sound_live(second));
    assert!(!system.runtime().sound_live(first));
    assert!(
        system.runtime_mut().play(PlayDesc::new(first)).is_err(),
        "the stale handle must not name the replacement"
    );
    assert!(system.runtime_mut().play(PlayDesc::new(second)).is_ok());
}

#[test]
fn voice_states_track_pause_resume_and_stop() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(256), 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound))
        .expect("voice");

    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert_eq!(status.position_frames, 0);
    assert_eq!(status.sound, Some(sound));
    assert!(!status.looping);
    assert_eq!(status.volume, 1.0);
    assert_eq!(status.pitch, 1.0);

    system.pump(32).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert_eq!(status.position_frames, 32);

    system.runtime_mut().pause(voice).expect("pause is queued");
    assert_eq!(
        system.runtime().voice_state(voice).expect("state").state,
        VoiceState::Playing,
        "the pause applies at the next block boundary"
    );
    system.pump(32).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Paused);
    assert_eq!(
        status.position_frames, 32,
        "a paused voice does not advance"
    );

    system
        .runtime_mut()
        .pause(voice)
        .expect("pausing twice is fine");
    system.pump(32).expect("pump");
    assert_eq!(
        system.runtime().voice_state(voice).expect("state").state,
        VoiceState::Paused
    );

    system
        .runtime_mut()
        .resume(voice)
        .expect("resume is queued");
    system.pump(32).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert_eq!(status.position_frames, 64);

    system.runtime_mut().stop(voice).expect("stop is queued");
    system.pump(32).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Finished);
    assert_eq!(status.position_frames, 0, "a stop resets the position");
    assert_eq!(
        system.runtime_mut().stop(voice).unwrap_err(),
        RuntimeError::NotFound,
        "there is nothing live left to stop"
    );
}

#[test]
fn voices_finish_and_their_slots_are_recycled() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&[0.5], 1, 48_000)
        .expect("one-frame sound");
    let mut voices = Vec::with_capacity(MAX_VOICES as usize);
    for _ in 0..MAX_VOICES {
        voices.push(
            system
                .runtime_mut()
                .play(PlayDesc::new(sound))
                .expect("a free voice"),
        );
    }
    assert_eq!(
        system.runtime_mut().play(PlayDesc::new(sound)).unwrap_err(),
        RuntimeError::Capacity,
        "the voice pool is exactly MAX_VOICES"
    );

    system.pump(32).expect("pump");
    let stats = system.runtime().stats();
    assert_eq!(stats.voices_started, u64::from(MAX_VOICES));
    assert_eq!(stats.voices_finished, u64::from(MAX_VOICES));
    for &voice in &voices {
        assert_eq!(
            system.runtime().voice_state(voice).expect("state").state,
            VoiceState::Finished
        );
    }

    let recycled = system
        .runtime_mut()
        .play(PlayDesc::new(sound))
        .expect("a finished slot is reusable");
    assert_ne!(voices[0], recycled, "the generation must change");
    assert_eq!(
        system
            .runtime()
            .voice_state(voices[0])
            .expect("the old handle stays readable")
            .state,
        VoiceState::Finished,
        "the old handle names the finished voice, never its successor"
    );
    assert_eq!(
        system.runtime().voice_state(recycled).expect("state").state,
        VoiceState::Playing,
        "the new handle names the new voice"
    );
}

#[test]
fn a_full_command_queue_reports_busy_and_recovers_after_a_drain() {
    let mut system = System::new_offline(48_000, 64).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(256), 1, 48_000)
        .expect("sound");
    let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES as usize);
    for _ in 0..MAX_VOICES {
        voices.push(
            system
                .runtime_mut()
                .play(PlayDesc::new(sound))
                .expect("voice"),
        );
    }
    // 64 plays, then three command rounds of 64: the queue's 256 slots
    // fill exactly.
    for &voice in &voices {
        system.runtime_mut().pause(voice).expect("pause is queued");
    }
    for &voice in &voices {
        system
            .runtime_mut()
            .resume(voice)
            .expect("resume is queued");
    }
    for &voice in &voices {
        system
            .runtime_mut()
            .set_voice_volume(voice, 0.5)
            .expect("volume is queued");
    }
    assert_eq!(
        system.runtime_mut().stop(voices[0]).unwrap_err(),
        RuntimeError::Busy,
        "the queue is full"
    );
    assert!(system.runtime().stats().commands_rejected >= 1);

    system.pump(64).expect("the pump drains the queue");
    system
        .runtime_mut()
        .stop(voices[0])
        .expect("the queue is empty again");
    system.pump(64).expect("pump");
    let status = system.runtime().voice_state(voices[0]).expect("state");
    assert_eq!(status.state, VoiceState::Finished);
    assert_eq!(status.position_frames, 0);
    assert_eq!(
        system
            .runtime()
            .voice_state(voices[1])
            .expect("state")
            .volume,
        0.5,
        "the queued volume landed"
    );
}

#[test]
fn removing_a_sound_stops_its_voices_at_the_next_block() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(512), 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound))
        .expect("voice");
    system.pump(32).expect("pump");

    system
        .runtime_mut()
        .remove_sound(sound)
        .expect("removal is accepted");
    system
        .runtime_mut()
        .remove_sound(sound)
        .expect("repeating the removal is fine until it is collected");
    assert!(!system.runtime().sound_live(sound), "retired immediately");
    assert_eq!(
        system.runtime_mut().play(PlayDesc::new(sound)).unwrap_err(),
        RuntimeError::NotFound
    );

    system.pump(32).expect("pump");
    assert_eq!(
        system.runtime().voice_state(voice).expect("state").state,
        VoiceState::Finished,
        "the render side stopped the orphaned voice"
    );
    let stats = system.runtime().stats();
    assert_eq!(stats.sounds_retired, 1);
    assert_eq!(stats.voices_orphaned, 1);
    assert_eq!(stats.voices_finished, 1);

    // Adding the replacement collects the retired slot, and the old
    // handle must not name anything any more.
    let replacement = system
        .runtime_mut()
        .add_sound(&dyadic(64), 1, 48_000)
        .expect("the slot is reusable");
    assert_ne!(replacement, sound);
    assert_eq!(
        system.runtime_mut().remove_sound(sound).unwrap_err(),
        RuntimeError::NotFound,
        "a collected handle names nothing"
    );
}

#[test]
fn volumes_clamp_and_buses_are_validated() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(64), 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound).with_volume(7.5).with_pitch(100.0))
        .expect("out-of-range values are clamped, not rejected");
    system.pump(1).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.volume, 1.0, "volume clamps to 1");
    assert_eq!(status.pitch, mikan_audio_sys::MAX_PITCH, "pitch clamps");

    let runtime = system.runtime_mut();
    assert_eq!(
        runtime.set_bus_volume(BUS_COUNT, 1.0).unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime
            .set_bus_volume(mikan_audio_sys::BUS_SFX, f32::NAN)
            .unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime.set_master_volume(f32::NAN).unwrap_err(),
        RuntimeError::BadArgument
    );
    runtime.set_bus_volume(BUS_MUSIC, 0.25).expect("valid bus");
    runtime.set_master_volume(2.0).expect("clamped");
    assert_eq!(
        runtime.set_voice_volume(voice, f32::NAN).unwrap_err(),
        RuntimeError::BadArgument
    );
    assert_eq!(
        runtime.set_voice_pitch(voice, 0.0).unwrap_err(),
        RuntimeError::BadArgument
    );
    runtime.set_voice_volume(voice, 0.25).expect("queued");
    runtime.set_voice_pitch(voice, 2.0).expect("queued");
    system.pump(1).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.volume, 0.25);
    assert_eq!(status.pitch, 2.0);
}

#[test]
fn looping_voices_never_finish_on_their_own() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&dyadic(8), 1, 48_000)
        .expect("short sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound).with_looping(true))
        .expect("voice");
    system.pump(32).expect("pump");
    system.pump(32).expect("pump");
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert!(status.looping);
    assert_eq!(
        status.position_frames, 8,
        "64 frames is 8 whole loops; the cursor rests on the wrap boundary"
    );
}
