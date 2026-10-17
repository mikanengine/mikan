//! Bit-for-bit cross-checks of the native mixer against the independent
//! Rust oracle in `mikan_audio_sys::reference`.
//!
//! Every test renders through the offline device — the same code path a
//! real device's callback drives — and compares the resulting samples
//! exactly. The test signals are chosen so that every intermediate value
//! is an exact binary fraction, which makes any interpolation, gain, or
//! rounding difference observable.

use mikan_audio_sys::reference::{self, Voice as ReferenceVoice};
use mikan_audio_sys::{PlayDesc, System, VoiceState};

/// Interleaved samples drawn from eighths: every value is exactly
/// representable in f32.
fn dyadic(frames: usize, channels: usize) -> Vec<f32> {
    (0..frames * channels)
        .map(|i| ((i % 16) as f32 - 7.5) / 8.0)
        .collect()
}

/// Pumps one whole block, drains it, and appends the device-format
/// samples to `out`.
fn pump_block(system: &mut System, block: u32, out: &mut Vec<i16>) {
    system.pump(block).expect("pump");
    let mut buffer = vec![0_i16; block as usize * 2];
    let read = system.read_s16(&mut buffer).expect("read");
    assert_eq!(read, block as usize);
    out.extend_from_slice(&buffer[..read * 2]);
}

/// Renders `blocks` blocks and returns every device-format sample.
fn render(system: &mut System, block: u32, blocks: usize) -> Vec<i16> {
    let mut native = Vec::with_capacity(block as usize * blocks * 2);
    for _ in 0..blocks {
        pump_block(system, block, &mut native);
    }
    native
}

/// The oracle's pipeline: clip, then quantize.
fn expected(accumulated: &[f32]) -> Vec<i16> {
    accumulated
        .iter()
        .map(|&sample| reference::to_s16(reference::clip(sample)))
        .collect()
}

#[test]
fn a_single_voice_matches_the_reference_exactly() {
    let block = 32_u32;
    let blocks = 16_usize;
    let samples = dyadic(200, 1);
    let mut system = System::new_offline(48_000, block).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&samples, 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound).with_volume(0.5))
        .expect("voice");

    let native = render(&mut system, block, blocks);

    let mut oracle = ReferenceVoice::new(samples, 1, 48_000, false);
    let mut accumulated = vec![0.0_f32; block as usize * blocks * 2];
    for chunk in accumulated.chunks_mut(block as usize * 2) {
        oracle.mix_into(chunk, 48_000, 0.5, 1.0);
    }
    assert_eq!(native, expected(&accumulated));

    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Finished);
    assert_eq!(
        status.position_frames, 200,
        "a finished voice sits at its end"
    );
}

#[test]
fn two_voices_mix_in_slot_order_with_buses_and_master_like_the_reference() {
    let block = 24_u32;
    let blocks = 20_usize;
    let mono = dyadic(120, 1);
    let stereo = dyadic(90, 2);
    let mut system = System::new_offline(48_000, block).expect("offline system");
    let runtime = system.runtime_mut();
    runtime.set_master_volume(0.5).expect("master");
    runtime
        .set_bus_volume(mikan_audio_sys::BUS_SFX, 0.5)
        .expect("sfx bus");
    let mono_sound = runtime.add_sound(&mono, 1, 44_100).expect("mono sound");
    let stereo_sound = runtime.add_sound(&stereo, 2, 48_000).expect("stereo sound");
    let mono_voice = runtime
        .play(PlayDesc::new(mono_sound).with_volume(0.5).with_pitch(0.75))
        .expect("mono voice");
    let stereo_voice = runtime
        .play(PlayDesc::new(stereo_sound).with_volume(1.0).with_pitch(1.5))
        .expect("stereo voice");

    let native = render(&mut system, block, blocks);

    // The native gain chain, in the native order: volume * bus * master.
    let mono_gain = 0.5 * 0.5 * 0.5;
    let stereo_gain = 1.0 * 0.5 * 0.5;
    let mut mono_oracle = ReferenceVoice::new(mono, 1, 44_100, false);
    let mut stereo_oracle = ReferenceVoice::new(stereo, 2, 48_000, false);
    let mut accumulated = vec![0.0_f32; block as usize * blocks * 2];
    for chunk in accumulated.chunks_mut(block as usize * 2) {
        // Voice slots mix in index order, and both were minted fresh.
        mono_oracle.mix_into(chunk, 48_000, mono_gain, 0.75);
        stereo_oracle.mix_into(chunk, 48_000, stereo_gain, 1.5);
    }
    assert_eq!(native, expected(&accumulated));

    let status = system.runtime().voice_state(mono_voice).expect("state");
    assert_eq!(status.state, VoiceState::Finished);
    assert_eq!(
        status.position_frames, 120,
        "a shorter sound at a lower rate still ends at its frame count"
    );
    let status = system.runtime().voice_state(stereo_voice).expect("state");
    assert_eq!(status.state, VoiceState::Finished);
    assert_eq!(status.position_frames, 90);
}

#[test]
fn a_looping_voice_wraps_like_the_reference() {
    let block = 3_u32;
    let blocks = 20_usize;
    let samples = dyadic(8, 1);
    let mut system = System::new_offline(48_000, block).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&samples, 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound).with_looping(true))
        .expect("voice");

    let native = render(&mut system, block, blocks);

    let mut oracle = ReferenceVoice::new(samples, 1, 48_000, true);
    let mut accumulated = vec![0.0_f32; block as usize * blocks * 2];
    for chunk in accumulated.chunks_mut(block as usize * 2) {
        oracle.mix_into(chunk, 48_000, 1.0, 1.0);
    }
    assert_eq!(native, expected(&accumulated));

    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert_eq!(status.position_frames, 4, "60 frames wrap modulo 8 frames");
}

#[test]
fn mid_run_controls_apply_at_block_boundaries_like_the_reference() {
    let block = 16_u32;
    let phase_blocks = 4_usize;
    let samples = dyadic(400, 1);
    let mut system = System::new_offline(48_000, block).expect("offline system");
    let sound = system
        .runtime_mut()
        .add_sound(&samples, 1, 48_000)
        .expect("sound");
    let voice = system
        .runtime_mut()
        .play(PlayDesc::new(sound).with_volume(0.25))
        .expect("voice");

    let mut oracle = ReferenceVoice::new(samples, 1, 48_000, false);
    let mut native = Vec::new();
    let mut accumulated = Vec::new();
    let mix_one_block =
        |oracle: &mut ReferenceVoice, accumulated: &mut Vec<f32>, volume: f32, pitch: f32| {
            let start = accumulated.len();
            accumulated.resize(start + block as usize * 2, 0.0);
            oracle.mix_into(&mut accumulated[start..], 48_000, volume, pitch);
        };

    // Phase 1: the values given at play time.
    for _ in 0..phase_blocks {
        pump_block(&mut system, block, &mut native);
        mix_one_block(&mut oracle, &mut accumulated, 0.25, 1.0);
    }

    // Phase 2: both queued before the next block, so from its first frame
    // the new values apply.
    system
        .runtime_mut()
        .set_voice_volume(voice, 0.5)
        .expect("volume queued");
    system
        .runtime_mut()
        .set_voice_pitch(voice, 0.5)
        .expect("pitch queued");
    for _ in 0..phase_blocks {
        pump_block(&mut system, block, &mut native);
        mix_one_block(&mut oracle, &mut accumulated, 0.5, 0.5);
    }

    // Phase 3: a paused block renders silence and consumes nothing.
    system.runtime_mut().pause(voice).expect("pause queued");
    pump_block(&mut system, block, &mut native);
    accumulated.resize(accumulated.len() + block as usize * 2, 0.0);

    // Phase 4: resume and keep going.
    system.runtime_mut().resume(voice).expect("resume queued");
    for _ in 0..2 {
        pump_block(&mut system, block, &mut native);
        mix_one_block(&mut oracle, &mut accumulated, 0.5, 0.5);
    }

    assert_eq!(native, expected(&accumulated));
    let status = system.runtime().voice_state(voice).expect("state");
    assert_eq!(status.state, VoiceState::Playing);
    assert_eq!(status.position_frames, 112);
}

#[test]
fn the_quantizer_rounds_as_documented() {
    assert_eq!(reference::to_s16(0.0), 0);
    assert_eq!(reference::to_s16(1.0), 32_767);
    assert_eq!(reference::to_s16(-1.0), -32_767);
    assert_eq!(reference::to_s16(2.0), 32_767);
    assert_eq!(reference::to_s16(-2.0), -32_767);
    assert_eq!(reference::to_s16(f32::INFINITY), 32_767);
    assert_eq!(reference::to_s16(f32::NEG_INFINITY), -32_767);
    assert_eq!(reference::to_s16(f32::NAN), 0);
    // Ties round upward: -0.5 maps to -16383, not -16384.
    assert_eq!(reference::to_s16(0.5), 16_384);
    assert_eq!(reference::to_s16(-0.5), -16_383);
    assert_eq!(reference::clip(1.5), 1.0);
    assert_eq!(reference::clip(-1.5), -1.0);
    assert_eq!(reference::clip(f32::NAN), 0.0);
    assert_eq!(reference::clip(0.25), 0.25);
}
