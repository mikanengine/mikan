/*
 * audio_backend.c — the C device backend for the Mikan audio module.
 *
 * Two device kinds sit behind the `mikan_audio_backend` handle:
 *
 *   - MIKAN_AUDIO_BACKEND_DEVICE: a real output device opened through the
 *     vendored miniaudio library. miniaudio runs the audio thread, calls
 *     `mikan_audio__device_data` with a f32 interleaved buffer at the
 *     requested rate, and converts that buffer to the hardware format
 *     itself; this file only owns the device, the callback wiring, and the
 *     status the notification callback maintains. miniaudio's silent "null"
 *     backend is deliberately rejected as "no device": a machine without
 *     audio hardware must not silently discard the mix.
 *
 *   - MIKAN_AUDIO_BACKEND_OFFLINE: a deterministic device driven by
 *     `mikan_audio_backend_pump` on the caller's thread. It renders one
 *     block through the callback, converts the f32 mix into interleaved s16
 *     — the same "device format" idea, in miniature — and appends it to a
 *     bounded ring buffer that `mikan_audio_backend_read_s16` drains. A
 *     block that does not fit is dropped and counted, which is what makes
 *     overrun behavior testable without a sound card.
 *
 * This file is compiled as C99 with warnings-as-errors; it must not use
 * C11 atomics (the header stays C99), so the counters the audio thread and
 * the control thread share go through the small compiler-atomic helpers
 * below.
 */

#include "mikan_audio.h"
#include "mikan_miniaudio_config.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_MSC_VER)
#include <intrin.h>
#endif
#include "miniaudio.h"

/* Frames the offline ring holds: `block_frames * this` bounds the queue of
 * undrained device output. */
#define MIKAN_AUDIO_RING_BLOCKS 8u

/* Longest device name miniaudio reports, plus the terminator. */
#define MIKAN_AUDIO_DEVICE_NAME_MAX 257u

/* ------------------------------------------------------------------ */
/* Cross-thread flags (the device thread writes; control reads)       */
/* ------------------------------------------------------------------ */

#if defined(_MSC_VER)
static void mikan_audio__flag_set(volatile long *flag, long value) {
    (void)_InterlockedExchange(flag, value);
}
static long mikan_audio__flag_get(volatile long *flag) {
    return _InterlockedCompareExchange(flag, 0, 0);
}
static void mikan_audio__counter_add(volatile long *counter, long delta) {
    (void)_InterlockedExchangeAdd(counter, delta);
}
static long mikan_audio__counter_get(volatile long *counter) {
    return _InterlockedCompareExchange(counter, 0, 0);
}
#else
static void mikan_audio__flag_set(volatile long *flag, long value) {
    __atomic_store_n(flag, value, __ATOMIC_RELEASE);
}
static long mikan_audio__flag_get(volatile long *flag) {
    return __atomic_load_n(flag, __ATOMIC_ACQUIRE);
}
static void mikan_audio__counter_add(volatile long *counter, long delta) {
    (void)__atomic_add_fetch(counter, delta, __ATOMIC_RELAXED);
}
static long mikan_audio__counter_get(volatile long *counter) {
    return __atomic_load_n(counter, __ATOMIC_RELAXED);
}
#endif

/* ------------------------------------------------------------------ */
/* The backend                                                        */
/* ------------------------------------------------------------------ */

struct mikan_audio_backend {
    uint32_t kind;
    uint32_t sample_rate;    /* the rate render callbacks run at */
    uint32_t block_frames;   /* frames per render call */
    mikan_audio_render_fn render;
    void *user;

    char backend_name[64];
    char device_name[MIKAN_AUDIO_DEVICE_NAME_MAX];

    /* Offline device state (control thread only). */
    float *scratch;          /* block_frames * CHANNELS float samples */
    int16_t *ring;           /* ring_capacity frames of two s16 samples */
    uint32_t ring_capacity;  /* frames */
    uint32_t ring_head;      /* next sample to read (in samples) */
    uint32_t ring_fill;      /* buffered samples */
    uint32_t overruns;

    /* Shared with the device thread. */
    volatile long running;      /* 0/1 */
    volatile long reroutes;
    volatile long interruptions;

    /* Real device state. */
    int device_open;
    ma_device device;
};

/* Clamps to -1..1 (NaN becomes silence) and quantizes to s16 the way the
 * Rust reference quantizer does: nearest, half away from zero upward. */
static int16_t mikan_audio__to_s16(float sample) {
    float clamped;
    if (sample >= -1.0f && sample <= 1.0f) {
        clamped = sample;
    } else if (sample > 1.0f) {
        clamped = 1.0f;
    } else if (sample < -1.0f) {
        clamped = -1.0f;
    } else {
        clamped = 0.0f; /* NaN */
    }
    return (int16_t)floorf(clamped * 32767.0f + 0.5f);
}

/* ------------------------------------------------------------------ */
/* The miniaudio device callbacks                                     */
/* ------------------------------------------------------------------ */

static void mikan_audio__device_data(ma_device *device, void *output, const void *input,
                                     ma_uint32 frame_count) {
    struct mikan_audio_backend *backend = (struct mikan_audio_backend *)device->pUserData;
    (void)input;
    if (backend == NULL || backend->render == NULL || output == NULL || frame_count == 0) {
        return;
    }
    backend->render(backend->user, (float *)output, (uint32_t)frame_count);
}

static void mikan_audio__device_notification(const ma_device_notification *notification) {
    struct mikan_audio_backend *backend;
    if (notification == NULL || notification->pDevice == NULL) {
        return;
    }
    backend = (struct mikan_audio_backend *)notification->pDevice->pUserData;
    if (backend == NULL) {
        return;
    }
    switch (notification->type) {
    case ma_device_notification_type_started:
        mikan_audio__flag_set(&backend->running, 1);
        break;
    case ma_device_notification_type_stopped:
        mikan_audio__flag_set(&backend->running, 0);
        break;
    case ma_device_notification_type_rerouted:
        mikan_audio__counter_add(&backend->reroutes, 1);
        break;
    case ma_device_notification_type_interruption_began:
        mikan_audio__counter_add(&backend->interruptions, 1);
        break;
    case ma_device_notification_type_interruption_ended:
    case ma_device_notification_type_unlocked:
        break;
    default:
        break;
    }
}

/* ------------------------------------------------------------------ */
/* Creation                                                           */
/* ------------------------------------------------------------------ */

static int mikan_audio__check_rate(uint32_t sample_rate) {
    return sample_rate >= 8000u && sample_rate <= 192000u;
}

static int mikan_audio__device_open(struct mikan_audio_backend *backend) {
    ma_device_config config = ma_device_config_init(ma_device_type_playback);
    ma_result result;
    const char *backend_name;

    config.playback.format = ma_format_f32;
    config.playback.channels = MIKAN_AUDIO_CHANNELS;
    config.sampleRate = backend->sample_rate;
    config.periodSizeInFrames = backend->block_frames;
    config.dataCallback = mikan_audio__device_data;
    config.notificationCallback = mikan_audio__device_notification;
    config.pUserData = backend;

    result = ma_device_init(NULL, &config, &backend->device);
    if (result != MA_SUCCESS) {
        return MIKAN_AUDIO_NO_DEVICE;
    }

    /* A machine with no audio hardware must fail here, not silently mix
     * into miniaudio's discarding null device; the caller falls back to
     * the offline kind explicitly. */
    if (backend->device.pContext == NULL || backend->device.pContext->backend == ma_backend_null) {
        ma_device_uninit(&backend->device);
        return MIKAN_AUDIO_NO_DEVICE;
    }

    backend->device_open = 1;
    backend->sample_rate = (uint32_t)backend->device.sampleRate;

    backend_name = ma_get_backend_name(backend->device.pContext->backend);
    if (backend_name != NULL) {
        snprintf(backend->backend_name, sizeof(backend->backend_name), "%s", backend_name);
    }
    if (ma_device_get_name(&backend->device, ma_device_type_playback, backend->device_name,
                           sizeof(backend->device_name), NULL) != MA_SUCCESS ||
        backend->device_name[0] == '\0') {
        snprintf(backend->device_name, sizeof(backend->device_name), "%s", "default");
    }
    return MIKAN_AUDIO_OK;
}

static int mikan_audio__offline_open(struct mikan_audio_backend *backend) {
    size_t scratch_samples = (size_t)backend->block_frames * MIKAN_AUDIO_CHANNELS;
    size_t ring_samples;

    backend->ring_capacity = backend->block_frames * MIKAN_AUDIO_RING_BLOCKS;
    ring_samples = (size_t)backend->ring_capacity * MIKAN_AUDIO_CHANNELS;

    backend->scratch = (float *)malloc(scratch_samples * sizeof(float));
    backend->ring = (int16_t *)malloc(ring_samples * sizeof(int16_t));
    if (backend->scratch == NULL || backend->ring == NULL) {
        free(backend->scratch);
        free(backend->ring);
        backend->scratch = NULL;
        backend->ring = NULL;
        return MIKAN_AUDIO_OUT_OF_MEMORY;
    }
    snprintf(backend->backend_name, sizeof(backend->backend_name), "%s", "offline");
    snprintf(backend->device_name, sizeof(backend->device_name), "%s", "offline");
    return MIKAN_AUDIO_OK;
}

int mikan_audio_backend_create(const mikan_audio_backend_desc *desc,
                               mikan_audio_render_fn render, void *user,
                               mikan_audio_backend **out_backend) {
    struct mikan_audio_backend *backend;
    uint32_t block_frames;
    int status;

    if (desc == NULL || render == NULL || out_backend == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_backend = NULL;
    if (desc->kind != MIKAN_AUDIO_BACKEND_DEVICE && desc->kind != MIKAN_AUDIO_BACKEND_OFFLINE) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (desc->sample_rate != 0 && !mikan_audio__check_rate(desc->sample_rate)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    block_frames = desc->block_frames;
    if (block_frames == 0) {
        block_frames = 1024;
    }
    if (block_frames > 65536u) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }

    backend = (struct mikan_audio_backend *)calloc(1, sizeof(*backend));
    if (backend == NULL) {
        return MIKAN_AUDIO_OUT_OF_MEMORY;
    }
    backend->kind = desc->kind;
    backend->sample_rate =
        desc->sample_rate == 0 ? MIKAN_AUDIO_DEFAULT_SAMPLE_RATE : desc->sample_rate;
    backend->block_frames = block_frames;
    backend->render = render;
    backend->user = user;

    if (desc->kind == MIKAN_AUDIO_BACKEND_DEVICE) {
        status = mikan_audio__device_open(backend);
    } else {
        status = mikan_audio__offline_open(backend);
    }
    if (status != MIKAN_AUDIO_OK) {
        free(backend);
        return status;
    }
    *out_backend = backend;
    return MIKAN_AUDIO_OK;
}

void mikan_audio_backend_destroy(mikan_audio_backend *backend) {
    if (backend == NULL) {
        return;
    }
    /* Uninitializing a device joins its worker thread (miniaudio waits for
     * it in `ma_device_uninit`), so after this call the render callback
     * cannot run again — which is what makes freeing the runtime that the
     * callback points at safe afterwards. */
    if (backend->device_open) {
        ma_device_uninit(&backend->device);
        backend->device_open = 0;
    }
    free(backend->scratch);
    free(backend->ring);
    free(backend);
}

int mikan_audio_backend_start(mikan_audio_backend *backend) {
    if (backend == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!backend->device_open) {
        /* Offline: pumping is what renders; the flag is informational. */
        mikan_audio__flag_set(&backend->running, 1);
        return MIKAN_AUDIO_OK;
    }
    if (ma_device_start(&backend->device) != MA_SUCCESS) {
        return MIKAN_AUDIO_DEVICE_ERROR;
    }
    mikan_audio__flag_set(&backend->running, 1);
    return MIKAN_AUDIO_OK;
}

int mikan_audio_backend_stop(mikan_audio_backend *backend) {
    if (backend == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!backend->device_open) {
        mikan_audio__flag_set(&backend->running, 0);
        return MIKAN_AUDIO_OK;
    }
    if (ma_device_stop(&backend->device) != MA_SUCCESS) {
        return MIKAN_AUDIO_DEVICE_ERROR;
    }
    mikan_audio__flag_set(&backend->running, 0);
    return MIKAN_AUDIO_OK;
}

void mikan_audio_backend_get_info(const mikan_audio_backend *backend,
                                  mikan_audio_backend_info *out_info) {
    if (backend == NULL || out_info == NULL) {
        return;
    }
    out_info->kind = backend->kind;
    out_info->sample_rate = backend->sample_rate;
    out_info->channels = MIKAN_AUDIO_CHANNELS;
    out_info->backend_name = backend->backend_name;
    out_info->device_name = backend->device_name;
}

int mikan_audio_backend_is_running(const mikan_audio_backend *backend, uint32_t *out_running) {
    if (backend == NULL || out_running == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_running = mikan_audio__flag_get((volatile long *)&backend->running) != 0 ? 1u : 0u;
    return MIKAN_AUDIO_OK;
}

int mikan_audio_backend_events(const mikan_audio_backend *backend, uint32_t *out_reroutes,
                               uint32_t *out_interruptions) {
    if (backend == NULL || out_reroutes == NULL || out_interruptions == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_reroutes = (uint32_t)mikan_audio__counter_get((volatile long *)&backend->reroutes);
    *out_interruptions = (uint32_t)mikan_audio__counter_get((volatile long *)&backend->interruptions);
    return MIKAN_AUDIO_OK;
}

/* ------------------------------------------------------------------ */
/* The offline device                                                 */
/* ------------------------------------------------------------------ */

int mikan_audio_backend_pump(mikan_audio_backend *backend, uint32_t frames) {
    uint32_t i;
    uint32_t samples;
    uint32_t ring_samples;

    if (backend == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (backend->kind != MIKAN_AUDIO_BACKEND_OFFLINE) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (frames == 0 || frames > backend->block_frames) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }

    backend->render(backend->user, backend->scratch, frames);

    samples = frames * MIKAN_AUDIO_CHANNELS;
    ring_samples = backend->ring_capacity * MIKAN_AUDIO_CHANNELS;
    if (samples > ring_samples - backend->ring_fill) {
        /* The program is not draining; drop the block and count it, the
         * way a real device would overrun. */
        backend->overruns += 1;
        return MIKAN_AUDIO_OK;
    }
    for (i = 0; i < samples; ++i) {
        backend->ring[(backend->ring_head + backend->ring_fill + i) % ring_samples] =
            mikan_audio__to_s16(backend->scratch[i]);
    }
    backend->ring_fill += samples;
    return MIKAN_AUDIO_OK;
}

int mikan_audio_backend_read_s16(mikan_audio_backend *backend, int16_t *out_frames,
                                 uint32_t frame_capacity, uint32_t *out_read) {
    uint32_t frames;
    uint32_t samples;
    uint32_t first;
    uint32_t ring_samples;

    if (backend == NULL || out_frames == NULL || out_read == NULL) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_read = 0;
    if (backend->kind != MIKAN_AUDIO_BACKEND_OFFLINE) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    frames = frame_capacity < backend->ring_fill / MIKAN_AUDIO_CHANNELS
                 ? frame_capacity
                 : backend->ring_fill / MIKAN_AUDIO_CHANNELS;
    if (frames == 0) {
        return MIKAN_AUDIO_OK;
    }
    samples = frames * MIKAN_AUDIO_CHANNELS;
    ring_samples = backend->ring_capacity * MIKAN_AUDIO_CHANNELS;
    first = samples;
    if (backend->ring_head + first > ring_samples) {
        first = ring_samples - backend->ring_head;
    }
    memcpy(out_frames, backend->ring + backend->ring_head, (size_t)first * sizeof(int16_t));
    if (samples > first) {
        memcpy(out_frames + first, backend->ring, (size_t)(samples - first) * sizeof(int16_t));
    }
    backend->ring_head = (backend->ring_head + samples) % ring_samples;
    backend->ring_fill -= samples;
    *out_read = frames;
    return MIKAN_AUDIO_OK;
}

uint32_t mikan_audio_backend_overruns(const mikan_audio_backend *backend) {
    if (backend == NULL) {
        return 0;
    }
    return backend->overruns;
}
