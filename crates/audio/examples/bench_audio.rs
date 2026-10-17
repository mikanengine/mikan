//! Times the mixer through the safe API: the cost of one render block, which
//! is what the audio callback (or the offline pump) runs, expressed per
//! output frame so it can be compared against the 48 kHz budget.
//!
//! Four rows, all on the deterministic offline backend so the measurement is
//! hardware-free and reproducible:
//!
//! 1. **1 voice** — the floor: block setup, one interpolated voice, the
//!    final clip pass, the ring store, and the s16 conversion.
//! 2. **16 voices** — what a busy game actually runs.
//! 3. **64 voices** — the full pool (every sample being summed 64 times,
//!    hard-clipping for most of each block).
//! 4. **64 voices at pitch 1.25** — the resampling path, where every voice
//!    reads between samples with a fractional position.
//!
//! The measured region is `update()` plus a full `drain_samples()`:
//! mix + clip + write to the ring + convert to the device's s16 format,
//! in one row, because that is the work a frame of audio costs. Control
//! calls are not timed here; they are a bounded queue push, and
//! `docs/audio.md` explains why.
//!
//! Run it through `scripts/bench_audio.sh`, or directly:
//!
//!     cargo run -p mikan-audio --release --example bench_audio [BLOCKS]

use std::hint::black_box;
use std::time::{Duration, Instant};

use mikan_audio::{AudioSystem, PlaySettings, SoundData};

/// The mixer's rate; the per-frame budget below is derived from it.
const SAMPLE_RATE: u32 = 48_000;
/// Frames per render block.
const BLOCK: u32 = 1024;

fn time(runs: u32, mut body: impl FnMut()) -> Duration {
    for _ in 0..3 {
        body();
    }
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed() / runs
}

fn report(label: &str, mean: Duration, frames: usize) {
    let per_frame = mean.as_nanos() as f64 / frames as f64;
    let frames_per_second = frames as f64 / mean.as_secs_f64();
    println!(
        "{label:<30} {per_frame:>8.1} ns/frame | {frames_per_second:>10.0} frames/s | {:>7.0}x real time",
        frames_per_second / f64::from(SAMPLE_RATE),
    );
}

/// A one-second looping tone: a 64-frame triangle ramp at 0.2 amplitude,
/// repeated. Deterministic and cheap to generate.
fn tone() -> Vec<f32> {
    (0..SAMPLE_RATE)
        .map(|index| 0.2 * ((index % 64) as f32 / 32.0 - 1.0))
        .collect()
}

fn main() {
    let blocks: usize = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(4096);
    let runs: u32 = 16;
    let frames = blocks * BLOCK as usize;
    let budget_ns = 1.0e9 / f64::from(SAMPLE_RATE);

    println!(
        "audio bench: {blocks} blocks x {BLOCK} frames at {SAMPLE_RATE} Hz stereo, {runs} runs (budget {budget_ns:.0} ns/frame)"
    );

    for (voices, pitch) in [(1_usize, 1.0_f32), (16, 1.0), (64, 1.0), (64, 1.25)] {
        let mut system = AudioSystem::new_offline(SAMPLE_RATE, BLOCK).expect("offline system");
        let samples = SoundData::from_samples(tone(), 1, SAMPLE_RATE).expect("valid tone");
        let sound = system.load_sound(&samples).expect("load the tone");
        for _ in 0..voices {
            system
                .play(
                    sound,
                    PlaySettings::new().with_looping(true).with_pitch(pitch),
                )
                .expect("a free voice");
        }

        let mut scratch = vec![0_i16; (BLOCK * 2) as usize];
        let mean = time(runs, || {
            for _ in 0..blocks {
                system.update().expect("offline render");
                black_box(system.drain_samples(&mut scratch).expect("drain"));
            }
            black_box(scratch.last());
        });
        assert_eq!(
            system.overruns(),
            0,
            "a draining bench never overruns the ring"
        );
        report(&format!("{voices} voice(s), pitch {pitch}"), mean, frames);
    }
}
