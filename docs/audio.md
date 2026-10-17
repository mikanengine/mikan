# Audio: from WAV files to mixed output

Mikan's audio system turns decoded sample data into either sound on a device or
deterministic blocks on the offline backend. This document is the design
record behind ADR 0014: what each layer owns, what the ABI promises, what the
mixer can and cannot do, and what has actually been measured.

## Crates and layers

| Crate | Language | Owns |
| --- | --- | --- |
| `mikan-audio-sys` | C + C++ | the device backend (C), the mixer runtime (C++), the vendored miniaudio, the raw FFI mirrors, and the cross-check reference |
| `mikan-assets` (`sound`) | Rust | the strict WAV decoder that produces `SoundData` |
| `mikan-audio` | Rust | the safe API: `AudioSystem`, handles, errors, the offline loop |
| `mikan` | Rust | re-exports for applications |
| `games/demo2d` | Rust | the `--audio` lane that proves the whole path |

The native split follows the same rule as every other module: **C owns the
low-level foundation** — here that means the operating-system resource, not
arithmetic; the backend initializes the device, owns the output callback and
the offline ring, and never interprets samples. **C++ owns the state machine
with real invariants** — voice lifecycle, mixing, resampling, gain routing —
behind one opaque handle. **Rust owns everything else**: the safe API, handles
with generation checks, error mapping, the asset-pipeline hookup, and the
tests. The two native layers do not call each other's data types: C calls one
C++ function (`mikan_audio_runtime_render`) with an opaque pointer, at block
boundaries and from the render side only.

## The ABI contract

`crates/audio-sys/src/mikan_audio.h` is the source of truth; the Rust mirror
restates it. The contract, in full:

- **Two opaque handles.** `mikan_audio_backend` (C) and `mikan_audio_runtime`
  (C++). Every entry point is `extern "C"`, returns a status code, and takes
  plain data: pointers plus lengths, integers, floats, and the two handles.
  No C++ types, no exceptions (the C++ translation unit is built with
  `-fno-exceptions -fno-rtti`), no allocation failures that can escape as
  anything but `OUT_OF_MEMORY`.
- **Threads.** The runtime has exactly one control thread (the thread that
  calls its entry points) and exactly one render side — either the device
  callback's thread or, offline, the thread that calls `pump`. Control-side
  entry points may be called from the control thread only; `render` and the
  apply path may be called from the render side only; `add_sound`/`remove`
  *commands* are the only crossing, and they cross through one bounded queue.
- **Ownership.** `add_sound` copies the caller's samples; the runtime owns
  them until the slot is reclaimed. The backend owns the runtime *pointer*
  only — it never frees it. The caller owns both handles.
- **Shutdown order is part of the contract.** The backend is destroyed
  *before* the runtime, because destroying the backend stops and joins the
  audio thread; destroying the runtime first would leave a callback pointing
  at freed memory. `mikan-audio-sys`'s `System` enforces the order with its
  field declaration order, and there is a test for it.
- **No realtime promise is taken on faith.** The render path allocates
  nothing, calls no system API, takes no lock, and never loops unboundedly:
  at most one pass over 64 voices and at most `block_frames` samples each.

## The mixer

The runtime is a fixed-size mixer, deliberately: fixed pools make the worst
case the documented case, and the audio thread never waits for memory.

| Pool | Size | Exhausted behavior |
| --- | --- | --- |
| Sound slots | 256 | `add_sound` → `CAPACITY` |
| Voices | 64 | `play` → `CAPACITY` |
| Command queue | 256 commands | control call → `BUSY` (nothing changes) |

- **Format.** `f32`, stereo, at the system's rate (default 48 000 Hz,
  documented range 8 000–192 000). Mono sources are widened; other channel
  counts are rejected (`UNSUPPORTED`).
- **Resampling.** Linear interpolation between source frames, done per voice
  per sample, so a voice's step is
  `(source_rate / output_rate) * pitch`. Pitch is clamped to `0.25..=4.0`.
- **Gain.** Voice volume is clamped to `0..=1` and stored as one atomic; bus
  (SFX, Music) and master volumes are also atoms. The sample path is
  `sample * voice_volume * bus_volume * master_volume`, summed, then **hard
  clipped** to `[-1, 1]` and converted to the device format.
- **Commands.** `play`, `pause`, `resume`, `stop`, `set volume`, `set pitch`
  enqueue one command each; the render side drains the queue at block
  boundaries, so a command's effect is visible from the *next* block. Bus and
  master gains skip the queue — a single atomic store is already safe.
- **The clipping and capacity answers are predictable.** Summing 64 voices at
  full gain clips at the same threshold every block; the 65th `play` fails
  with a named error instead of stealing a voice. Neither behavior depends on
  timing.

### Handles

Sounds and voices are `{index, generation: u32 >= 1}` pairs. The generation
increments every time a slot is reused, which is what makes a stale handle
*safe* rather than dangerous:

| Situation | `play`/`add`/control | `voice_status` |
| --- | --- | --- |
| live handle | works | current state |
| recycled slot | `NOT_FOUND` | `Finished`, zeroed payload |
| index out of range, generation 0, or a made-up future generation | `NOT_FOUND` | `NOT_FOUND` |

A stale voice reports `Finished` because a game loop polls voice state at
frame rate and "the voice you asked about is over" is the honest answer; a
stale sound cannot be played, so it must be an error. `stop` rewinds the
position to 0 and moves the voice to `Finished`; the slot is reusable by the
next `play`, which bumps the generation and makes the old handle stale.

### Sound lifetime

Removal is two-phase so that a control call never touches render-side state:

1. control: `remove_sound` marks the slot `RETIRED` and queues `REMOVE`;
   `play` on it fails immediately (`NOT_FOUND`), and it is idempotent until
   reclaimed;
2. render: the remove command stops every voice playing that sound (counting
   them as orphaned) and marks the slot `DEAD`;
3. control: `collect` frees `DEAD` slots to `FREE`; the next `add_sound` may
   reuse the index with a new generation.

On the offline backend, `collect` runs inside `update()`; on the device path a
later control call (or an explicit `collect`) does it. Until it runs, the slot
stays occupied — the honest failure mode is `CAPACITY`, never corruption.

## The offline backend

The offline backend is not a test shim: it is the second implementation of the
same contract, and it is what makes the system verifiable on any machine.

- `new_offline(rate, block)` creates backend + runtime with no device at all.
- `pump(frames)` renders exactly one block (`1..=block_frames`) through the
  **same** `mikan_audio_runtime_render` the device callback calls, and appends
  it to an internal ring (8 blocks).
- `read_s16(out)` drains interleaved stereo samples, converting to `i16` the
  way the device format expects.
- If a pump arrives with a full ring, the block is dropped and counted: the
  offline backend never blocks, never grows, and never hides the loss —
  `overruns()` reports it.

The demo drains every rendered frame, so its offline runs show zero overruns;
a game that pumps faster than it drains will see the counter rise, which is
the intended signal.

## The safe layer

`mikan-audio` contains no `unsafe` and no device knowledge beyond
`new_device`. The API in one screen:

```rust
use mikan::{AudioSystem, Bus, PlaySettings, SoundData};

let mut audio = match AudioSystem::new_device(48_000, 1024) {
    Ok(system) => system,
    Err(_) => AudioSystem::new_offline(48_000, 1024)?, // no sound card
};
let data = SoundData::load_wav("blip.wav")?;   // real asset pipeline
let blip = audio.load_sound(&data)?;           // one decode, one copy
let voice = audio.play(blip, PlaySettings::new()
    .with_bus(Bus::Sfx)
    .with_volume(0.5)
    .with_pitch(1.25))?;
audio.pause(voice)?;
audio.resume(voice)?;
audio.set_bus_volume(Bus::Sfx, 0.8)?;
audio.update()?;                               // no-op on the device path
```

- **Errors are the game-facing truth.** `NoDevice` (use the offline backend),
  `DeviceError`, `InvalidSampleRate`, `InvalidArgument` (non-finite gain),
  `Unsupported`, `Capacity`, `NotFound`, `Busy` (retry after `update`),
  `OutOfMemory`, `OfflineOnly`, and `Asset` (the decoder's own error).
  Values that merely exceed a range are clamped; values that cannot mean
  anything (NaN) are rejected. Nothing panics.
- **`AudioSystem` is neither `Send` nor `Sync`.** The native contract is one
  control thread plus the render side, and the type system says so instead of
  a comment. Games that want audio off-thread own that decision explicitly.
- **`update()` is the loop hook that is right on both paths**: on a device it
  does nothing, offline it renders one block and reclaims unloaded sounds.
- **`drain_samples`/`overruns` are offline-only** and say so
  (`OfflineOnly`) on a device, where the samples went to the sound card.

## The asset pipeline: the WAV subset

The decoder lives in `mikan-assets` next to the PNG and sprite-sheet code, so
audio files go through the same "real files, validated strictly, committed by
a generator" discipline.

Accepted: RIFF/WAVE, little-endian, `fmt` and `data` in either order, unknown
chunks skipped with padding honored, `WAVE_FORMAT_EXTENSIBLE` with the standard
PCM or IEEE-float GUID. Sample formats: PCM 8-bit unsigned, 16/24/32-bit
signed, IEEE float 32/64. Channels: mono or stereo. Sample rate: 8 000–192 000
Hz. Frame count: up to `2^24` (16 777 216 frames — about 5.8 minutes at
48 kHz), allocated with a fallible reservation.

Rejected, each with its own error and **no partial decode**: non-RIFF data,
truncated or inconsistent chunks, `block_align`/sizes that disagree with the
format, more than 2 channels, 12/20-bit or otherwise unpacked layouts,
ADPCM and the other compressed tags, sample rates outside the range, and
files larger than the frame cap.

## Engine integration and the demo

The engine does not own an `AudioSystem`. The facade (`mikan`) re-exports the
whole safe API, and the game constructs the system explicitly at startup,
because "this machine has no output device" is a decision only the game can
make. That is the documented pattern: try `new_device`, fall back to
`new_offline` (and say so) when it fails.

`demo2d --audio` demonstrates it end to end: the lane opens the device (or,
with a warning, the offline backend when the device cannot start), loads
`assets/blip.wav` — generated deterministically by `scripts/generate_assets.py`
and committed — through `SoundData::load_wav` + `load_sound`, and plays it on
every edge from standing to moving, with pitch 1.25 upward, 0.75 downward, and
1.0 otherwise. `update()`/drain run once per displayed frame, and shutdown logs
one summary line:

```
audio: 2 plays, 2 voices started, 2 finished, 425 blocks mixed, 0 overruns, backend offline
```

`scripts/verify_audio.sh` asserts the whole sequence under Xvfb — backend
line, decode line, both blips at the right pitches, matching counters, zero
overruns, Escape quitting with status 0 — and is wired into `scripts/verify.sh`
and the CI smoke job. Because the fallback path is part of the product, the
verification is meaningful on a machine with no sound card (which is every CI
runner today).

## The measurement

The driver for this module is the architecture directive, not a profiled
bottleneck (ADR 0014): no speedup over some Rust baseline is claimed, and none
is needed. What is measured is the practical contract of any mixer — **can the
worst case keep up with realtime?** — on the deterministic offline backend, so
the numbers do not depend on hardware:

- `scripts/bench_audio.sh [BLOCKS]` runs
  `crates/audio/examples/bench_audio.rs` in release mode: 16 timed runs of
  4096 blocks × 1024 frames (about 87 s of audio per run), 3 warm-up runs, rows
  for 1, 16, and 64 looping voices at pitch 1.0 and 64 at pitch 1.25
  (resampling). The measured region is `update()` + a full `drain_samples()`:
  mix, clip, write, and convert.
- The budget is **20 833 ns per output frame** (`1e9 / 48 000`); the recorded
  run on this machine (Intel Core i3-N305) measured 12.2 ns/frame for one voice,
  62.6 for 16, **198.8 for the full 64-voice pool, and 192.4 for 64 voices at
  pitch 1.25** — the worst case the pool allows uses under 1 % of one core's
  realtime budget, and even that row is 105× faster than realtime. The exact
  command, the raw log path, and the context are recorded in `docs/STATUS.md`.
- One caveat stated up front: the rows are *per output frame*, and the
  offline path also adds the ring store and the s16 conversion that a device
  callback's copy would normally do, so the device path is not slower than the
  recorded number.

## Limitations and extensions

Documented as out of scope for this milestone, none stubbed:

- **Streaming playback.** A decoded sound is copied into a mixer slot whole,
  so the frame cap is also a memory consideration (16.7 M frames is ~128 MB as
  f32 stereo). Long music tracks need a streaming voice and a second slot
  lifecycle.
- **DSP effects, reverb, spatialization.** No filters, no busses beyond the two
  gain groups, no 3D position model. These need a per-voice effect graph and a
  listener model, both absent.
- **Recording / capture devices.** The backend is output-only.
- **Format breadth.** One WAV subset, deliberately strict. Ogg/MP3 would need
  a vendored decoder and a policy decision about which; there is no "try
  anyway" path that produces noise.
- **A second mixer rate.** The system runs at one output rate; a device that
  prefers another rate is resampled by its own backend policy or fails
  honestly at open time.
- **Device-path verification on real hardware.** This machine has no sound
  card, so the first honest device test is still pending; see
  `docs/STATUS.md`'s limitations section for the exact state.
