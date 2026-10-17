# 0014. Audio system: a C device backend over vendored miniaudio and a C++ mixer runtime

- Status: Accepted
- Date: 2026-10-17

## Context

The milestone after input is the **audio system**: the layer that turns decoded
sample data into sound on one machine and into deterministic blocks on another.
The roadmap in `docs/ARCHITECTURE.md` did not define audio as Phase 9; the
project owner directed this milestone explicitly, the same kind of
owner-directed amendment as Luau scripting (ADR 0011), and this ADR is where
that decision is recorded: the repository's own policy (`docs/ffi.md`) says a
native module introduced by an architecture decision must say so in an ADR.

The architecture directive from Phase 5 stands: Rust + C++ forming the engine
core, C as the low-level foundation. Audio is the first milestone where the
low-level foundation is *not* arithmetic over caller-owned data — it is a
device: an operating-system resource with a callback that runs on a thread
Rust never sees. That changes what "C owns the foundation" has to mean.

Open questions the milestone had to answer:

1. **Where does the Rust/C/C++ line sit?** The honest options: all-Rust (a
   `crosstool`-style crate), native-everything, or a split. The split chosen
   follows the data: **C owns the device backend** — context and device
   initialization, output buffers, the realtime output callback, the
   deterministic offline ring, and the narrow C ABI; **C++ owns the playback
   runtime and mixer** — voice lifecycle, sample mixing and resampling, buses
   and master gain, and the lock-free command path between the two sides; and
   **Rust owns everything else** — the safe API, the asset-pipeline hookup,
   handles with generation checks, error mapping, and the tests. The mixer is a
   per-sample, per-voice state machine, which is exactly where C++ earned its
   place in the scene and input modules.
2. **Where does the device come from?** Writing per-platform backends (ALSA,
   CoreAudio, WASAPI, JACK) is a large surface. A vendored library was the
   honest choice: **miniaudio 0.11.25**, single-header C, public domain or
   MIT-0 — the "existing library consumed on its own terms" case in
   `docs/ffi.md`, with the version and its SHA-256 recorded in-tree. Its
   support for a silent "null" backend was **deliberately disabled** in the
   vendored config: a machine with no device must report `NO_DEVICE` and let
   the caller choose the offline backend, never silently pretend to play.
3. **How do the two threads talk?** The audio callback cannot block, cannot
   allocate, and cannot take a lock the control thread holds. The design is a
   bounded single-producer/single-consumer command queue (256 commands) with
   atomic head/tail indices, drained at block boundaries on the render side,
   plus atomic snapshots (voice state, position) the control thread reads
   without synchronization. Bus and master gains are single atomic stores —
   no command at all.
4. **What is a handle?** Voice handles are `(index, generation)` pairs, so a
   recycled slot cannot be confused with its predecessor; the same shape was
   later applied to sound handles, because slot reuse made the earlier plain
   index-plus-one ids name a *different* sound after an unload-and-load cycle.
   Stale voices report `FINISHED` with a zeroed payload (the state
   query cannot fail on the render thread); stale sounds report `NOT_FOUND`
   (adding or playing a retired sound is an error, not a silent success).
5. **What happens when a sound is unloaded while voices play it?** Removal is
   two-phase: the control side marks the slot `RETIRED` and queues a remove
   command; the render side stops its orphaned voices, marks the slot `DEAD`,
   and a later `collect` frees it. `unload_sound` is idempotent until the slot
   is reclaimed; the old handle then reports `NOT_FOUND` even if the slot was
   reused, which is the whole point of the generation.
6. **What stays out?** Streaming playback (the whole decoded sound must fit
   in memory), DSP effects, reverb, 3D spatialization, recording, and
   multi-format decoding beyond one strict WAV subset. Each of those has no
   compatible half-interface today; they are documented as future work rather
   than stubbed.

The measurement question is answered the same way the scene and input
milestones answered it: the driver is **the architecture directive, not a
profiled bottleneck**, and the recorded benchmark reports the mixer's cost
against the realtime budget as measured — including the rows where the honest
answer is "this is the worst case the pool allows".

## Decision

1. **Two new crates, following the required structure in `docs/ffi.md`:**
   - `crates/audio-sys` — the native module: `src/audio_backend.c` (C, the
     device backend and the offline ring), `src/audio_runtime.cpp` (C++, the
     mixer), `src/miniaudio_impl.c` (the single translation unit that compiles
     the vendored header), `src/mikan_audio.h` (the contract: threading,
     ownership, handles, shutdown order), `third_party/miniaudio/` (the pinned
     vendored source and its hash), a `build.rs` that compiles each source
     with warnings-as-errors and `-fno-exceptions -fno-rtti` for C++, the
     status/struct mirrors, RAII handles that enforce the shutdown order, and
     `reference` — an independent Rust mirror of the mixer's arithmetic.
   - `crates/audio` — the safe wrapper (no `unsafe`): `AudioSystem` owns the
     backend and the runtime, opens the device or the offline ring, and maps
     statuses to `AudioError`; `Sound`/`Voice` are copyable
     generation-checked handles; `PlaySettings` and `Bus` carry the knobs.
2. **The C backend's surface is the device, not the music.** One opaque
   `mikan_audio_backend`: create (with a sample rate and block size, or in
   offline mode), start, stop, destroy, and — offline only — `pump`,
   `read_s16`, and an overrun counter. In device mode the backend owns the
   miniaudio device and its callback; the callback calls one C++ function
   (`mikan_audio_runtime_render`) with the runtime pointer it was handed. The
   C backend never interprets samples.
3. **The C++ runtime owns everything about what comes out.** One opaque
   `mikan_audio_runtime`: fixed pools (256 sounds, 64 voices, a 256-command
   queue), f32 stereo internally, linear-interpolation resampling for pitch
   and rate conversion, volumes and pitch clamped to their documented ranges,
   hard clipping at `[-1, 1]`, and the full voice state machine
   (`RESERVED/PLAYING/PAUSED/FINISHED` internally, published as
   `Playing/Paused/Finished`). It allocates in `add_sound` (one copy of the
   caller's samples) and nowhere else on the render path.
4. **The realtime path is checked, not promised.** The render function
   performs no allocation, no system call, no lock acquisition, and no
   unbounded loop: at most `block_frames` samples per voice, at most 64
   voices, commands drained up to the queue's capacity. The command queue is
   the only cross-thread structure; its memory is allocated at creation.
5. **Handles are generation-checked on both sides.** `(index, generation)`
   for sounds and voices; generation 0 and out-of-range indices are
   `NOT_FOUND`; a stale voice reports `FINISHED` with zeroed fields; a stale
   sound reports `NOT_FOUND`. Mutating commands re-validate the generation at
   apply time (`resolve_current_voice`), so a command queued for a recycled
   voice cannot land on its successor.
6. **Rust is the only bridge and adds engine semantics.** `AudioSystem`
   exposes `new_device`/`new_offline`, `load_sound`/`load_wav`,
   `unload_sound`, `play`/`pause`/`resume`/`stop`,
   `set_voice_volume`/`set_voice_pitch`/`set_bus_volume`/`set_master_volume`,
   `voice_status`, `update`, and offline-only `drain_samples`/`overruns`. It
   is deliberately neither `Send` nor `Sync`: the native contract is one
   control thread plus the render side, and the type system says so.
7. **The asset pipeline gains one format, strictly.** `mikan-assets` decodes
   RIFF/WAVE little-endian PCM (8/16/24/32-bit integer) and IEEE float
   (32/64-bit), mono or stereo, 8 kHz–192 kHz, including
   `WAVE_FORMAT_EXTENSIBLE` with the standard PCM/float GUIDs. Everything
   else — ADPCM, more channels, 20/12-bit layouts, non-WAVE files — fails
   with a specific error before any partial decode is returned. Loading is a
   decode plus one copy into the mixer; playing is neither.
8. **Tests are the contract at every level.** `audio-sys` tests the backend,
   the runtime, the raw FFI, and — the oracle — compares the C++ mixer's
   output against an independent Rust implementation **bit for bit** in the
   s16 domain on exact dyadic inputs. `mikan-audio` tests the safe API on the
   offline backend; `mikan-assets` tests the decoder against hand-built and
   corrupted files. No test requires a sound card; the device path is
   exercised to its documented `NO_DEVICE` outcome on hardware-free machines.
9. **A deterministic offline backend is part of the product, not the tests.**
   `pump`, `read_s16`, and a bounded ring with an overrun counter make the
   same mixer code runnable in CI, in headless environments, and for games
   that render audio offline. "No device" is a first-class, honest result.
10. **The demo demonstrates the whole path.** `demo2d --audio` opens the
    device (falling back to the offline backend with an explicit warning),
    loads a deterministic `blip.wav` produced by `scripts/generate_assets.py`
    through the real pipeline, plays it on movement edges with pitch following
    the direction, and summarizes the counters at shutdown.
    `scripts/verify_audio.sh` asserts all of that under Xvfb, wired into
    `scripts/verify.sh` and the CI smoke job.
11. **The measurement is reported as measured.** `scripts/bench_audio.sh`
    times the mixer through the safe API on the offline backend — 1, 16, and
    64 looping voices at pitch 1.0 and 64 at pitch 1.25 — and reports ns per
    output frame against the 20 833 ns realtime budget at 48 kHz. No speedup
    is claimed and none is needed: the module's driver is the architecture,
    and the numbers exist to show the worst case the pool allows.

## Consequences

Positive:

- Games get audio through a small safe API: load once, play many, with
  generation-checked handles and honest errors for every failure mode a game
  actually hits (no device, bad file, pool exhaustion, stale handle, full
  command queue).
- The same mixer runs on a real device and in the deterministic offline
  backend, so CI, headless environments, and this repository's own
  verification exercise the real code path with no sound card present.
- The realtime callback's constraints are enforced by construction — fixed
  pools, a pre-allocated command queue, no locks, no allocation, no I/O — and
  the cross-check makes the mixer's arithmetic verifiable independent of the
  device.
- The vendored miniaudio is one pinned file with its hash recorded; the null
  backend is off, so "no silent device" is a policy the code enforces rather
  than a convention.

Negative:

- **No Valgrind run is recorded yet.** This machine has no Valgrind, and
  installing it needs interactive authorization; `docs/STATUS.md` records the
  exact command to run and the pending status instead of claiming a result.
- **The device path was exercised only to `NO_DEVICE` locally.** The machine
  has no ALSA/PulseAudio/JACK and no `/dev/snd`, so the real output path is
  verified by code review, by the offline backend sharing the mixer, and by
  CI where the same fallback applies; the first machine with a sound card is
  still the first honest device test.
- The whole decoded sound must fit in memory (no streaming), the mixer is
  fixed at one output format internally, and there is no DSP, reverb, or 3D
  spatialization. All documented, none stubbed.
- In `mikan-audio-sys`, dropping the backend before the runtime is a
  *documented* ordering requirement; the Rust wrapper enforces it with struct
  field order, not with the type system, because the two handles are designed
  to be wired together.
