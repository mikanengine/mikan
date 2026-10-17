//! Raw-ABI tests: forged handles, null pointers, and other inputs the safe
//! wrapper makes impossible to produce. They call the C ABI directly on
//! purpose, because that is the layer that must reject them.

use std::ptr;

use mikan_audio_sys::{
    AudioPlayDesc, AudioSound, AudioVoice, AudioVoiceState, BAD_ARGUMENT, MAX_SOUNDS, MAX_VOICES,
    NOT_FOUND, OK, UNSUPPORTED, mikan_audio_runtime_add_sound, mikan_audio_runtime_create,
    mikan_audio_runtime_destroy, mikan_audio_runtime_pause, mikan_audio_runtime_play,
    mikan_audio_runtime_remove_sound, mikan_audio_runtime_render, mikan_audio_runtime_sound_live,
    mikan_audio_runtime_voice_state,
};

fn empty_state() -> AudioVoiceState {
    AudioVoiceState {
        state: 0,
        sound: AudioSound {
            index: 0,
            generation: 0,
        },
        looping: 0,
        volume: 0.0,
        pitch: 0.0,
        position_frames: 0,
    }
}

fn forged_voice(index: u32, generation: u32) -> AudioVoice {
    AudioVoice { index, generation }
}

fn forged_sound(index: u32, generation: u32) -> AudioSound {
    AudioSound { index, generation }
}

#[test]
fn forged_voice_handles_are_rejected() {
    let mut runtime = ptr::null_mut();
    // SAFETY: the out pointer is a live local.
    assert_eq!(
        unsafe { mikan_audio_runtime_create(48_000, &mut runtime) },
        OK
    );
    assert!(!runtime.is_null());

    let forged = [
        forged_voice(u32::MAX, 1),
        forged_voice(MAX_VOICES, 1),
        forged_voice(0, 0),
        // The slot's generation is still zero, so this is "from the future".
        forged_voice(0, 1),
    ];
    for voice in forged {
        let mut state = empty_state();
        // SAFETY: the runtime is live; `state` is a live local.
        assert_eq!(
            unsafe { mikan_audio_runtime_voice_state(runtime, voice, &mut state) },
            NOT_FOUND
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { mikan_audio_runtime_pause(runtime, voice) },
            NOT_FOUND
        );
    }

    // Null handles and outputs are bad arguments, never crashes.
    let mut state = empty_state();
    // SAFETY: pointers are null or live locals; nothing is dereferenced
    // beyond the documented rules.
    unsafe {
        assert_eq!(
            mikan_audio_runtime_voice_state(ptr::null(), forged_voice(0, 1), &mut state),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_voice_state(runtime, forged_voice(0, 1), ptr::null_mut()),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_pause(ptr::null_mut(), forged_voice(0, 1)),
            BAD_ARGUMENT
        );
        mikan_audio_runtime_destroy(runtime);
    }
}

#[test]
fn forged_sound_handles_are_rejected() {
    let mut runtime = ptr::null_mut();
    // SAFETY: the out pointer is a live local.
    assert_eq!(
        unsafe { mikan_audio_runtime_create(48_000, &mut runtime) },
        OK
    );

    let samples = [0.5_f32; 8];
    let forged = [
        forged_sound(u32::MAX, 1),
        forged_sound(MAX_SOUNDS, 1),
        forged_sound(0, 0),
        forged_sound(0, 1),
    ];
    let live = std::cell::Cell::new(u32::MAX);
    for sound in forged {
        // SAFETY: the runtime is live; `live` is a live local.
        unsafe {
            assert_eq!(
                mikan_audio_runtime_sound_live(runtime, sound, live.as_ptr()),
                OK
            );
            assert_eq!(live.get(), 0);
            assert_eq!(mikan_audio_runtime_remove_sound(runtime, sound), NOT_FOUND);
        }
        let desc = AudioPlayDesc {
            sound,
            bus: 0,
            looping: 0,
            volume: 1.0,
            pitch: 1.0,
        };
        let mut voice = forged_voice(0, 0);
        // SAFETY: `desc` and `voice` are live locals.
        assert_eq!(
            unsafe { mikan_audio_runtime_play(runtime, &desc, &mut voice) },
            NOT_FOUND
        );
    }

    // Registers one sound so the argument checks can be exercised on a
    // live runtime; the handle is then valid.
    let mut sound = forged_sound(0, 0);
    // SAFETY: all pointers are live locals or the slice's buffer.
    unsafe {
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, ptr::null(), 4, 1, 48_000, &mut sound),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 4, 1, 48_000, ptr::null_mut()),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 0, 1, 48_000, &mut sound),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 4, 3, 48_000, &mut sound),
            UNSUPPORTED
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 4, 1, 7_999, &mut sound),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 4, 1, 192_001, &mut sound),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_audio_runtime_add_sound(runtime, samples.as_ptr(), 8, 1, 48_000, &mut sound),
            OK
        );
        assert_ne!(sound.generation, 0);
        let mut live = 0_u32;
        assert_eq!(
            mikan_audio_runtime_sound_live(runtime, sound, &mut live),
            OK
        );
        assert_eq!(live, 1);
        mikan_audio_runtime_destroy(runtime);
    }
}

#[test]
fn null_handles_are_no_ops_and_a_null_render_is_ignored() {
    let mut runtime = ptr::null_mut();
    // SAFETY: every argument is null or a live local; the module documents
    // all of these as rejected or ignored.
    unsafe {
        assert_eq!(
            mikan_audio_runtime_create(48_000, ptr::null_mut()),
            BAD_ARGUMENT
        );
        assert_eq!(mikan_audio_runtime_create(100, &mut runtime), BAD_ARGUMENT);
        assert!(runtime.is_null());
        mikan_audio_runtime_destroy(ptr::null_mut());
        mikan_audio_runtime_destroy(runtime);

        let desc = AudioPlayDesc {
            sound: forged_sound(0, 1),
            bus: 0,
            looping: 0,
            volume: 1.0,
            pitch: 1.0,
        };
        let mut voice = forged_voice(0, 0);
        assert_eq!(
            mikan_audio_runtime_play(ptr::null_mut(), &desc, &mut voice),
            BAD_ARGUMENT
        );

        // Rendering through a null runtime is ignored, exactly like a
        // null user pointer in the backend's callback.
        let mut frames = [0.25_f32; 8];
        mikan_audio_runtime_render(ptr::null_mut(), frames.as_mut_ptr(), 4);
        assert!(frames.iter().all(|&sample| sample == 0.25));
        mikan_audio_runtime_render(ptr::null_mut(), ptr::null_mut(), 0);
    }
}
