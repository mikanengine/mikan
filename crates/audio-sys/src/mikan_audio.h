/*
 * mikan_audio.h — the C ABI of the Mikan audio module.
 *
 * The module has two native layers behind this one header:
 *
 *   - the C device backend (`audio_backend.c`): opens and owns the output
 *     device (through the vendored miniaudio library) or the deterministic
 *     offline device, converts the mixed f32 frames into the device format,
 *     and drives the render callback on its own thread (device) or on the
 *     caller's thread (offline pump);
 *   - the C++ runtime (`audio_runtime.cpp`): the sound registry, the voice
 *     pool, playback control, the command queue, and the mixer.
 *
 * The Rust wrapper `mikan-audio` is the only intended consumer; it wires the
 * two layers together by handing `mikan_audio_runtime_render` to
 * `mikan_audio_backend_create` as the render callback.
 *
 * Threading contract
 * ------------------
 * Both handles follow the module's single-control-thread rule:
 *
 *   - every function that takes a non-const handle is a *control* function:
 *     it must be called from one thread at a time (the wrapper encodes this
 *     with `&mut self`), and it never blocks on the render thread;
 *   - the render callback runs on the backend's thread for the device kind
 *     and on the control thread for the offline kind; it must not call any
 *     function declared here;
 *   - the *query* functions (`mikan_audio_runtime_voice_state`,
 *     `mikan_audio_runtime_sound_live`, `mikan_audio_runtime_stats_into`,
 *     `mikan_audio_backend_is_running`) only read atomics and may be called
 *     from any thread.
 *
 * Shutdown order
 * --------------
 * The backend owns the thread that calls into the runtime, so the backend
 * must be destroyed *before* the runtime: `mikan_audio_backend_destroy`
 * stops the device and joins its thread (miniaudio's `ma_device_uninit`
 * waits for the worker to terminate), after which the runtime is no longer
 * reachable from the backend and may be freed.
 *
 * Ownership
 * ---------
 * Every handle is allocated by its matching `create` and freed by its
 * matching `destroy`, exactly once. `mikan_audio_runtime_add_sound` copies
 * the sample data into runtime-owned memory; the caller keeps ownership of
 * the buffer it passed and may free it as soon as the call returns.
 */

#ifndef MIKAN_AUDIO_H
#define MIKAN_AUDIO_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------ */
/* Constants                                                          */
/* ------------------------------------------------------------------ */

/** Mixer output channels: every render callback frame is stereo f32. */
#define MIKAN_AUDIO_CHANNELS 2u

/** The mixer's output rate when none is requested. */
#define MIKAN_AUDIO_DEFAULT_SAMPLE_RATE 48000u

/** Concurrent registered sounds; exceeding this is a capacity error. */
#define MIKAN_AUDIO_MAX_SOUNDS 256u

/** Concurrent voices; `play` fails with a capacity error when full. */
#define MIKAN_AUDIO_MAX_VOICES 64u

/** Buses the runtime mixes through, each with its own gain. */
#define MIKAN_AUDIO_BUS_COUNT 2u

/** Outstanding control commands the queue holds before `play`/setters
 *  report a busy error; drained by every render block. */
#define MIKAN_AUDIO_QUEUE_CAPACITY 256u

/** Smallest pitch (playback rate) factor a voice accepts. */
#define MIKAN_AUDIO_MIN_PITCH 0.25f

/** Largest pitch (playback rate) factor a voice accepts. */
#define MIKAN_AUDIO_MAX_PITCH 4.0f

/* ------------------------------------------------------------------ */
/* Status codes                                                       */
/* ------------------------------------------------------------------ */

/** The call succeeded. */
#define MIKAN_AUDIO_OK 0
/** A null pointer, a non-finite float, or an out-of-range enum value. */
#define MIKAN_AUDIO_BAD_ARGUMENT 1
/** The device was opened but the operation failed (for example `start`). */
#define MIKAN_AUDIO_DEVICE_ERROR 2
/** No usable output device: no backend could be opened. */
#define MIKAN_AUDIO_NO_DEVICE 3
/** A pool is exhausted: no free sound slot or no free voice. */
#define MIKAN_AUDIO_CAPACITY 4
/** The voice or sound handle does not name a live object. */
#define MIKAN_AUDIO_NOT_FOUND 5
/** The data is outside the module's documented support (channels > 2). */
#define MIKAN_AUDIO_UNSUPPORTED 6
/** The command queue is full; retry after the next render block. */
#define MIKAN_AUDIO_BUSY 7
/** An allocation inside the module failed. */
#define MIKAN_AUDIO_OUT_OF_MEMORY 8

/* ------------------------------------------------------------------ */
/* The device backend (C)                                             */
/* ------------------------------------------------------------------ */

/** Opaque device backend handle; created and destroyed below. */
typedef struct mikan_audio_backend mikan_audio_backend;

/** `desc.kind`: a real output device through miniaudio. */
#define MIKAN_AUDIO_BACKEND_DEVICE 0u
/** `desc.kind`: the deterministic offline device, driven by `pump`. */
#define MIKAN_AUDIO_BACKEND_OFFLINE 1u

/**
 * The render callback the backend drives.
 *
 * `frames` holds `frame_count * MIKAN_AUDIO_CHANNELS` interleaved f32
 * samples to be *filled* (not accumulated into); `user` is the pointer
 * passed to `mikan_audio_backend_create`. The callback must not call any
 * function declared in this header. `frame_count` is never zero.
 */
typedef void (*mikan_audio_render_fn)(void *user, float *frames, uint32_t frame_count);

/** Backend creation parameters; zeroed fields select the defaults. */
typedef struct mikan_audio_backend_desc {
    /** One of `MIKAN_AUDIO_BACKEND_*`. */
    uint32_t kind;
    /** Render sample rate; 0 selects `MIKAN_AUDIO_DEFAULT_SAMPLE_RATE`. */
    uint32_t sample_rate;
    /** Frames per render call; 0 selects 1024. For the offline kind this
     *  is also the `pump` granularity and sizes the output ring. */
    uint32_t block_frames;
} mikan_audio_backend_desc;

/** Read-only backend facts, valid while the backend lives. */
typedef struct mikan_audio_backend_info {
    /** One of `MIKAN_AUDIO_BACKEND_*`. */
    uint32_t kind;
    /** The rate render callbacks actually run at. */
    uint32_t sample_rate;
    /** Always `MIKAN_AUDIO_CHANNELS`; present for the callers' checks. */
    uint32_t channels;
    /** The miniaudio backend name ("ALSA", "WASAPI", ...) or "offline". */
    const char *backend_name;
    /** The opened device name, or "offline". Never null; never freed. */
    const char *device_name;
} mikan_audio_backend_info;

/**
 * Creates a backend. For `MIKAN_AUDIO_BACKEND_DEVICE` this opens the
 * default output device; if no real backend initializes (or miniaudio
 * would fall back to its silent "null" device) the call fails with
 * `MIKAN_AUDIO_NO_DEVICE`, and the caller decides whether to use
 * `MIKAN_AUDIO_BACKEND_OFFLINE` instead. The offline kind allocates its
 * pump scratch and output ring and never fails on a machine property.
 *
 * `render` must stay valid until `destroy`; `user` is passed back to it.
 *
 * Returns `MIKAN_AUDIO_OK`, `MIKAN_AUDIO_BAD_ARGUMENT`,
 * `MIKAN_AUDIO_NO_DEVICE`, `MIKAN_AUDIO_DEVICE_ERROR`, or
 * `MIKAN_AUDIO_OUT_OF_MEMORY`.
 */
int mikan_audio_backend_create(const mikan_audio_backend_desc *desc,
                               mikan_audio_render_fn render, void *user,
                               mikan_audio_backend **out_backend);

/**
 * Stops the device (if running), joins its thread, and frees the backend.
 * Null is a no-op. After this call the render callback cannot run again,
 * which is what makes destroying the runtime afterwards sound.
 */
void mikan_audio_backend_destroy(mikan_audio_backend *backend);

/**
 * Starts the output device. For the device kind this starts the audio
 * thread; for the offline kind it only raises the status flag (pumping is
 * always allowed). Starting twice is a no-op. Returns `MIKAN_AUDIO_OK`,
 * `MIKAN_AUDIO_BAD_ARGUMENT`, or `MIKAN_AUDIO_DEVICE_ERROR`.
 */
int mikan_audio_backend_start(mikan_audio_backend *backend);

/** Stops the device; stopping a stopped backend is a no-op. */
int mikan_audio_backend_stop(mikan_audio_backend *backend);

/** Copies the backend's facts into `out_info`; either pointer may be null
 *  for a no-op. */
void mikan_audio_backend_get_info(const mikan_audio_backend *backend,
                                  mikan_audio_backend_info *out_info);

/** Writes 1 into `out_running` when the device is started, else 0. */
int mikan_audio_backend_is_running(const mikan_audio_backend *backend,
                                   uint32_t *out_running);

/**
 * Copies the device-event counters the miniaudio notification callback
 * maintains: reroutes (the device moved to another endpoint) and
 * interruption beginnings (sleep, device grab). Always zero for the
 * offline kind.
 */
int mikan_audio_backend_events(const mikan_audio_backend *backend, uint32_t *out_reroutes,
                               uint32_t *out_interruptions);

/**
 * Offline kind only: renders `frames` frames (1..=block_frames) through the
 * render callback, converts them to interleaved s16 in the device format,
 * and appends them to the output ring for `read_s16` to drain. A block
 * that does not fit is dropped and counted by `overruns`. Returns
 * `MIKAN_AUDIO_BAD_ARGUMENT` for the device kind or an out-of-range count.
 */
int mikan_audio_backend_pump(mikan_audio_backend *backend, uint32_t frames);

/**
 * Offline kind only: drains up to `frame_capacity` frames from the output
 * ring into `out_frames` (interleaved s16 stereo) and writes the number
 * drained into `out_read`. Partial drains are normal.
 */
int mikan_audio_backend_read_s16(mikan_audio_backend *backend, int16_t *out_frames,
                                 uint32_t frame_capacity, uint32_t *out_read);

/** Offline kind only: frames dropped over the backend's lifetime because
 *  the ring was full when a pump produced them. */
uint32_t mikan_audio_backend_overruns(const mikan_audio_backend *backend);

/* ------------------------------------------------------------------ */
/* The audio runtime (C++)                                            */
/* ------------------------------------------------------------------ */

/** Opaque runtime handle; created and destroyed below. */
typedef struct mikan_audio_runtime mikan_audio_runtime;

/** Bus 0: sound effects. */
#define MIKAN_AUDIO_BUS_SFX 0u
/** Bus 1: music. */
#define MIKAN_AUDIO_BUS_MUSIC 1u

/** `voice_state.state`: the voice is mixing (or was just requested). */
#define MIKAN_AUDIO_VOICE_PLAYING 1u
/** `voice_state.state`: paused; the position is preserved. */
#define MIKAN_AUDIO_VOICE_PAUSED 2u
/** `voice_state.state`: terminal: completed, stopped, or released because
 *  its sound was removed. */
#define MIKAN_AUDIO_VOICE_FINISHED 3u

/**
 * A voice handle. Never forged by the module: it is the pair the runtime
 * returned from `play`. The generation makes recycled slots detectable, so
 * a handle to a finished voice can never name a later, unrelated one.
 */
typedef struct mikan_audio_voice {
    /** Slot index, 0..MIKAN_AUDIO_MAX_VOICES-1. */
    uint32_t index;
    /** Slot generation; zero never names a voice. */
    uint32_t generation;
} mikan_audio_voice;

/**
 * A sound handle, minted by `mikan_audio_runtime_add_sound`. Like voice
 * handles, the pair makes recycled slots detectable: a handle from before a
 * removal can never name the sound that later reuses the slot.
 */
typedef struct mikan_audio_sound {
    /** Slot index, 0..MIKAN_AUDIO_MAX_SOUNDS-1. */
    uint32_t index;
    /** Slot generation; zero never names a sound. */
    uint32_t generation;
} mikan_audio_sound;

/** A voice's visible state (a snapshot; see the threading contract). */
typedef struct mikan_audio_voice_state {
    /** One of `MIKAN_AUDIO_VOICE_*`. */
    uint32_t state;
    /** The sound the voice plays; generation zero for none. */
    mikan_audio_sound sound;
    /** 1 when the voice loops. */
    uint32_t loop;
    /** The voice's gain, 0..1. */
    float volume;
    /** The voice's pitch factor, `MIKAN_AUDIO_MIN_PITCH`..`MIKAN_AUDIO_MAX_PITCH`. */
    float pitch;
    /** Frames consumed of the sound. A stop resets this to zero; a natural
     *  completion leaves it at the sound's frame count. */
    uint64_t position_frames;
} mikan_audio_voice_state;

/** Everything `play` needs to start one voice. */
typedef struct mikan_audio_play_desc {
    /** A live sound handle from `mikan_audio_runtime_add_sound`. */
    mikan_audio_sound sound;
    /** One of `MIKAN_AUDIO_BUS_*`. */
    uint32_t bus;
    /** Non-zero to loop forever. */
    uint32_t loop;
    /** Gain in 0..1; clamped. NaN or infinity is rejected. */
    float volume;
    /** Pitch factor in `MIKAN_AUDIO_MIN_PITCH`..`MIKAN_AUDIO_MAX_PITCH`;
     *  clamped. The voice's playback rate is
     *  `sound.sample_rate / output_rate * pitch`. */
    float pitch;
} mikan_audio_play_desc;

/** Runtime counters; every field is monotonic. */
typedef struct mikan_audio_runtime_stats {
    /** Voices that began playing. */
    uint64_t voices_started;
    /** Voices that reached a terminal state (completion, stop, or sound
     *  removal) after playing. */
    uint64_t voices_finished;
    /** Render blocks the mixer produced. */
    uint64_t blocks_mixed;
    /** Commands the render side applied. */
    uint64_t commands_applied;
    /** Control calls rejected because the command queue was full. */
    uint64_t commands_rejected;
    /** Sounds fully retired. */
    uint64_t sounds_retired;
    /** Live voices stopped early because their sound was removed. */
    uint64_t voices_orphaned;
} mikan_audio_runtime_stats;

/**
 * Creates a runtime that mixes at `sample_rate` (8000..192000). The
 * runtime starts empty: no sounds, no voices, master and bus gains at 1.
 */
int mikan_audio_runtime_create(uint32_t sample_rate, mikan_audio_runtime **out_runtime);

/**
 * Destroys a runtime and every buffer it owns. The backend that renders
 * into it must already be destroyed (see "Shutdown order"). Null is a
 * no-op.
 */
void mikan_audio_runtime_destroy(mikan_audio_runtime *runtime);

/**
 * Registers a sound by copying `frame_count * channels` f32 samples into
 * runtime-owned memory. `channels` must be 1 or 2 and `sample_rate` must
 * be 8000..192000; anything else is `MIKAN_AUDIO_UNSUPPORTED` or
 * `MIKAN_AUDIO_BAD_ARGUMENT`. Writes the new handle into `out_sound`.
 * Runs any pending `collect` first, then fails with `MIKAN_AUDIO_CAPACITY`
 * when all `MIKAN_AUDIO_MAX_SOUNDS` slots are taken.
 */
int mikan_audio_runtime_add_sound(mikan_audio_runtime *runtime, const float *samples,
                                  uint32_t frame_count, uint32_t channels,
                                  uint32_t sample_rate, mikan_audio_sound *out_sound);

/**
 * Retires a sound: new `play` calls for it are rejected immediately, its
 * live voices are stopped by the render side, and its sample buffer is
 * freed by the next `collect`. Idempotent while the handle names the
 * sound (including after it is acknowledged but before it is collected);
 * a handle whose slot was recycled, or one that never named a sound, is
 * `MIKAN_AUDIO_NOT_FOUND`. When the module was just created and the queue
 * is full the call fails with `MIKAN_AUDIO_BUSY` and changes nothing.
 */
int mikan_audio_runtime_remove_sound(mikan_audio_runtime *runtime, mikan_audio_sound sound);

/**
 * Frees the buffers of sounds whose retirement the render side has
 * acknowledged and returns their slots to the pool. Called automatically
 * by `add_sound`; exposed for callers that remove without adding.
 */
void mikan_audio_runtime_collect(mikan_audio_runtime *runtime);

/** Writes 1 when the handle names a registered, not-yet-retired sound,
 *  else 0. A stale handle — one whose slot was recycled — writes 0. */
int mikan_audio_runtime_sound_live(const mikan_audio_runtime *runtime, mikan_audio_sound sound,
                                   uint32_t *out_live);

/**
 * Allocates a voice for `desc` and hands it to the render side. Returns
 * `MIKAN_AUDIO_CAPACITY` when all voices are taken, `MIKAN_AUDIO_NOT_FOUND`
 * when the sound is unknown or retired, `MIKAN_AUDIO_BUSY` when the command
 * queue is full, `MIKAN_AUDIO_BAD_ARGUMENT` for a bad bus or a non-finite
 * volume, and `MIKAN_AUDIO_UNSUPPORTED` when the sound's channels are not 1
 * or 2. The voice reports `MIKAN_AUDIO_VOICE_PLAYING` immediately; it
 * begins mixing at the next render block. Each call allocates a fresh
 * voice; asking again for the same sound starts a second one.
 */
int mikan_audio_runtime_play(mikan_audio_runtime *runtime, const mikan_audio_play_desc *desc,
                             mikan_audio_voice *out_voice);

/** Pauses a live voice; the position is preserved. Returns
 *  `MIKAN_AUDIO_NOT_FOUND` for stale or unknown handles and `MIKAN_AUDIO_OK`
 *  when the voice was already paused. */
int mikan_audio_runtime_pause(mikan_audio_runtime *runtime, mikan_audio_voice voice);

/** Resumes a paused voice; playing voices are left alone. */
int mikan_audio_runtime_resume(mikan_audio_runtime *runtime, mikan_audio_voice voice);

/** Stops a live voice: it becomes `MIKAN_AUDIO_VOICE_FINISHED` and its
 *  position resets to zero. Stopping a finished voice is
 *  `MIKAN_AUDIO_NOT_FOUND`: the handle no longer names live state. */
int mikan_audio_runtime_stop(mikan_audio_runtime *runtime, mikan_audio_voice voice);

/** Sets a live or finished voice's gain (clamped to 0..1). The change
 *  applies from the next render block. */
int mikan_audio_runtime_set_voice_volume(mikan_audio_runtime *runtime,
                                         mikan_audio_voice voice, float volume);

/** Sets a live or finished voice's pitch (clamped to the documented
 *  range). The change applies from the next render block. */
int mikan_audio_runtime_set_voice_pitch(mikan_audio_runtime *runtime,
                                        mikan_audio_voice voice, float pitch);

/** Sets one bus's gain (clamped to 0..1); applied by the render side at
 *  the next block without a queued command. */
int mikan_audio_runtime_set_bus_volume(mikan_audio_runtime *runtime, uint32_t bus, float volume);

/** Sets the master gain (clamped to 0..1); applied by the render side at
 *  the next block without a queued command. */
int mikan_audio_runtime_set_master_volume(mikan_audio_runtime *runtime, float volume);

/**
 * Reads a voice's state. A handle that is stale (its slot was recycled)
 * reports `MIKAN_AUDIO_VOICE_FINISHED`: the voice it named is over. A
 * handle that never named a voice (index out of range, generation zero or
 * from the future) is `MIKAN_AUDIO_NOT_FOUND`.
 */
int mikan_audio_runtime_voice_state(const mikan_audio_runtime *runtime, mikan_audio_voice voice,
                                    mikan_audio_voice_state *out_state);

/** Copies the runtime's counters atomically into `out_stats`. */
void mikan_audio_runtime_stats_into(const mikan_audio_runtime *runtime,
                                    mikan_audio_runtime_stats *out_stats);

/**
 * The render entry point: mixes the runtime's voices into `frames`
 * (`frame_count * MIKAN_AUDIO_CHANNELS` interleaved f32 samples) and clips
 * the result to -1..1. `user` is the `mikan_audio_runtime *`. This is the
 * function to pass to `mikan_audio_backend_create`, and it may be called
 * directly by tests to drive the mixer deterministically.
 */
void mikan_audio_runtime_render(void *user, float *frames, uint32_t frame_count);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* MIKAN_AUDIO_H */
