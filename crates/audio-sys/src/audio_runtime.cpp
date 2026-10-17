/*
 * audio_runtime.cpp — the C++ playback runtime and mixer for the Mikan
 * audio module.
 *
 * This file owns everything between "the game asked for a sound" and "the
 * device got interleaved f32 frames":
 *
 *   - the sound registry: registered sample buffers with their frame
 *     counts, channel counts, and rates (1 or 2 channels; anything wider
 *     is rejected before it gets here);
 *   - the voice pool: MIKAN_AUDIO_MAX_VOICES fixed slots, each with a
 *     generation counter so a handle to a finished voice can never name a
 *     later, unrelated one;
 *   - the command protocol: control calls enqueue small POD commands into
 *     a bounded single-producer/single-consumer ring; the render side
 *     drains them at block boundaries. No lock is ever taken on the render
 *     path and no allocation happens there;
 *   - the mixer: every playing voice is resampled with linear
 *     interpolation, scaled by its gain, its bus's gain, and the master
 *     gain, summed into the output buffer, and the sum is clipped to
 *     -1..1.
 *
 * Threading
 * ---------
 * Control functions (everything taking a non-const runtime) run on one
 * control thread and only touch atomics plus the command queue's producer
 * side. The render functions (`render`, `apply`) run on the backend's
 * thread for a real device and on the control thread for the offline
 * pump; they only touch the queue's consumer side, the voice pool, and
 * atomics. Query functions read atomics only. That is the whole protocol;
 * the wrapper's `&mut self` on control methods is what makes the producer
 * single by construction.
 *
 * Lifecycle details worth their own note:
 *
 *   - stop resets the position to zero and moves the voice to FINISHED;
 *     the slot is only reusable once a later `play` claims it, which bumps
 *     the generation and invalidates the old handle;
 *   - removing a sound while the device runs is two-phase: the control
 *     side marks the slot RETIRED (new plays fail immediately), the render
 *     side stops that sound's voices and marks it DEAD, and the next
 *     `collect` frees the buffer and returns the slot to the pool. No
 *     reader can still hold the buffer after DEAD, which is what makes the
 *     free sound;
 *   - the mixer clips: a sum above 1 becomes 1. There is no limiter.
 *
 * This file is compiled as C++ with warnings-as-errors, without
 * exceptions and without RTTI; it must not let either cross the ABI.
 */

#include "mikan_audio.h"

#include <atomic>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <new>

namespace {

/* Internal voice states; the public constants in the header are a subset
 * with different numbers on purpose, so an accidental mix-up fails to
 * compile at the boundary instead of silently aliasing. */
enum : uint32_t {
    kVoiceSlotFree = 0,
    kVoiceSlotReserved = 1,
    kVoiceSlotPlaying = 2,
    kVoiceSlotPaused = 3,
    kVoiceSlotFinished = 4,
};

/* Sound slot states. */
enum : uint32_t {
    kSoundFree = 0,
    kSoundActive = 1,
    kSoundRetired = 2,
    kSoundDead = 3,
};

enum : uint32_t {
    kCommandPlay = 1,
    kCommandPause = 2,
    kCommandResume = 3,
    kCommandStop = 4,
    kCommandSetVolume = 5,
    kCommandSetPitch = 6,
    kCommandRemoveSound = 7,
};

/* The largest render block the mixer accepts; the backend caps blocks at
 * 65536 frames, this is only a defensive bound for direct callers. */
constexpr uint32_t kMaxRenderFrames = 1u << 20;

float clamp01(float value) {
    if (!(value == value)) { /* NaN */
        return 0.0f;
    }
    if (value < 0.0f) {
        return 0.0f;
    }
    if (value > 1.0f) {
        return 1.0f;
    }
    return value;
}

float clamp_pitch(float value) {
    if (!(value == value) || value < MIKAN_AUDIO_MIN_PITCH) {
        return MIKAN_AUDIO_MIN_PITCH;
    }
    if (value > MIKAN_AUDIO_MAX_PITCH) {
        return MIKAN_AUDIO_MAX_PITCH;
    }
    return value;
}

struct Command {
    uint32_t kind;
    uint32_t voice_index;
    uint32_t generation;
    uint32_t sound_index;      /* a sound slot for play/remove, else 0 */
    uint32_t sound_generation; /* the handle's generation for the above */
    uint32_t bus;
    uint32_t loop;
    float volume;
    float pitch;
};

/* Single-producer/single-consumer fixed ring. The producer (control) is
 * single by contract; so is the consumer (render), which is why plain
 * head/tail counters with release/acquire stores are enough. */
class CommandQueue {
  public:
    CommandQueue() : head_(0), tail_(0) {}

    bool has_space() const {
        uint32_t tail = tail_.load(std::memory_order_relaxed);
        uint32_t head = head_.load(std::memory_order_acquire);
        return tail - head < MIKAN_AUDIO_QUEUE_CAPACITY;
    }

    bool enqueue(const Command &command) {
        uint32_t tail = tail_.load(std::memory_order_relaxed);
        uint32_t head = head_.load(std::memory_order_acquire);
        if (tail - head >= MIKAN_AUDIO_QUEUE_CAPACITY) {
            return false;
        }
        entries_[tail % MIKAN_AUDIO_QUEUE_CAPACITY] = command;
        tail_.store(tail + 1, std::memory_order_release);
        return true;
    }

    bool dequeue(Command *out) {
        uint32_t head = head_.load(std::memory_order_relaxed);
        uint32_t tail = tail_.load(std::memory_order_acquire);
        if (head == tail) {
            return false;
        }
        *out = entries_[head % MIKAN_AUDIO_QUEUE_CAPACITY];
        head_.store(head + 1, std::memory_order_release);
        return true;
    }

    bool empty() const {
        return head_.load(std::memory_order_relaxed) == tail_.load(std::memory_order_acquire);
    }

  private:
    std::atomic<uint32_t> head_;
    std::atomic<uint32_t> tail_;
    Command entries_[MIKAN_AUDIO_QUEUE_CAPACITY];
};

struct SoundSlot {
    SoundSlot()
        : state(kSoundFree),
          generation(0),
          frames(0),
          channels(0),
          sample_rate(0),
          samples(nullptr) {}

    std::atomic<uint32_t> state;
    std::atomic<uint32_t> generation;
    std::atomic<uint32_t> frames;
    std::atomic<uint32_t> channels;
    std::atomic<uint32_t> sample_rate;
    std::atomic<const float *> samples;
};

struct VoiceSlot {
    VoiceSlot()
        : state(kVoiceSlotFree),
          generation(0),
          sound_index(0),
          sound_generation(0),
          bus(0),
          loop(0),
          volume(1.0f),
          pitch(1.0f),
          position_frames(0),
          samples(nullptr),
          frames(0),
          channels(0),
          base_ratio(1.0),
          position(0.0) {}

    /* Shared atomics. */
    std::atomic<uint32_t> state;
    std::atomic<uint32_t> generation;
    std::atomic<uint32_t> sound_index;
    std::atomic<uint32_t> sound_generation;
    std::atomic<uint32_t> bus;
    std::atomic<uint32_t> loop;
    std::atomic<float> volume;
    std::atomic<float> pitch;
    std::atomic<uint64_t> position_frames;

    /* Render-private state; written only while the slot is reserved or
     * live, read only by the render side. */
    const float *samples;
    uint32_t frames;
    uint32_t channels;
    double base_ratio; /* sound rate / output rate; pitch multiplies it */
    double position;   /* fractional cursor in the sound's frames */
};

} // namespace

/* ------------------------------------------------------------------ */
/* The runtime                                                        */
/* ------------------------------------------------------------------ */

struct mikan_audio_runtime {
    explicit mikan_audio_runtime(uint32_t rate)
        : output_rate(rate),
          master_volume(1.0f),
          stat_voices_started(0),
          stat_voices_finished(0),
          stat_blocks(0),
          stat_commands(0),
          stat_rejected(0),
          stat_sounds_retired(0),
          stat_orphaned(0) {
        for (uint32_t bus = 0; bus < MIKAN_AUDIO_BUS_COUNT; ++bus) {
            bus_volume[bus].store(1.0f, std::memory_order_relaxed);
        }
    }

    ~mikan_audio_runtime() {
        for (uint32_t i = 0; i < MIKAN_AUDIO_MAX_SOUNDS; ++i) {
            const float *samples = sounds[i].samples.load(std::memory_order_relaxed);
            if (samples != nullptr) {
                free(const_cast<float *>(samples));
                sounds[i].samples.store(nullptr, std::memory_order_relaxed);
            }
        }
    }

    uint32_t output_rate;
    SoundSlot sounds[MIKAN_AUDIO_MAX_SOUNDS];
    VoiceSlot voices[MIKAN_AUDIO_MAX_VOICES];
    CommandQueue queue;
    std::atomic<float> master_volume;
    std::atomic<float> bus_volume[MIKAN_AUDIO_BUS_COUNT];

    std::atomic<uint64_t> stat_voices_started;
    std::atomic<uint64_t> stat_voices_finished;
    std::atomic<uint64_t> stat_blocks;
    std::atomic<uint64_t> stat_commands;
    std::atomic<uint64_t> stat_rejected;
    std::atomic<uint64_t> stat_sounds_retired;
    std::atomic<uint64_t> stat_orphaned;

    /* ---- control-side helpers ---- */

    /* Resolves a live sound handle to its slot. Unlike voices, a stale
     * sound handle is simply invalid: a generation the slot has moved
     * past, a future one, and zero all fail. */
    bool resolve_sound(const mikan_audio_sound &sound, SoundSlot **out_slot) const {
        if (sound.index >= MIKAN_AUDIO_MAX_SOUNDS) {
            return false;
        }
        SoundSlot *slot = const_cast<SoundSlot *>(&sounds[sound.index]);
        if (sound.generation == 0 ||
            slot->generation.load(std::memory_order_acquire) != sound.generation) {
            return false;
        }
        *out_slot = slot;
        return true;
    }

    /* Whether a handle can still name state at all: in range, generation
     * not zero, not from the future. A stale-but-real handle passes; the
     * caller then finds the slot FINISHED (or its successor). */
    bool resolve_voice(const mikan_audio_voice &voice, VoiceSlot **out_slot) const {
        if (voice.index >= MIKAN_AUDIO_MAX_VOICES) {
            return false;
        }
        VoiceSlot *slot = const_cast<VoiceSlot *>(&voices[voice.index]);
        uint32_t generation = slot->generation.load(std::memory_order_acquire);
        if (voice.generation == 0 || voice.generation > generation) {
            return false;
        }
        *out_slot = slot;
        return true;
    }

    /* Like `resolve_voice`, but only a handle whose generation is the
     * slot's current one: queued commands for a voice that has since been
     * recycled must never land on its successor. */
    bool resolve_current_voice(const mikan_audio_voice &voice, VoiceSlot **out_slot) const {
        VoiceSlot *slot = nullptr;
        if (!resolve_voice(voice, &slot)) {
            return false;
        }
        if (voice.generation != slot->generation.load(std::memory_order_acquire)) {
            return false;
        }
        *out_slot = slot;
        return true;
    }

    /* Frees the buffers of sounds the render side has acknowledged. */
    void collect() {
        for (uint32_t i = 0; i < MIKAN_AUDIO_MAX_SOUNDS; ++i) {
            SoundSlot &slot = sounds[i];
            if (slot.state.load(std::memory_order_acquire) != kSoundDead) {
                continue;
            }
            const float *samples = slot.samples.load(std::memory_order_relaxed);
            if (samples != nullptr) {
                free(const_cast<float *>(samples));
                slot.samples.store(nullptr, std::memory_order_relaxed);
            }
            slot.frames.store(0, std::memory_order_relaxed);
            slot.channels.store(0, std::memory_order_relaxed);
            slot.sample_rate.store(0, std::memory_order_relaxed);
            slot.state.store(kSoundFree, std::memory_order_release);
        }
    }

    /* ---- render-side ---- */

    void apply(const Command &command) {
        switch (command.kind) {
        case kCommandPlay: {
            if (command.voice_index >= MIKAN_AUDIO_MAX_VOICES) {
                return;
            }
            VoiceSlot &slot = voices[command.voice_index];
            if (slot.state.load(std::memory_order_acquire) != kVoiceSlotReserved ||
                slot.generation.load(std::memory_order_acquire) != command.generation) {
                return;
            }
            SoundSlot *sound = nullptr;
            const mikan_audio_sound handle{command.sound_index, command.sound_generation};
            if (!resolve_sound(handle, &sound) ||
                sound->state.load(std::memory_order_acquire) != kSoundActive) {
                slot.position_frames.store(0, std::memory_order_release);
                slot.state.store(kVoiceSlotFinished, std::memory_order_release);
                return;
            }
            slot.samples = sound->samples.load(std::memory_order_acquire);
            slot.frames = sound->frames.load(std::memory_order_relaxed);
            slot.channels = sound->channels.load(std::memory_order_relaxed);
            slot.base_ratio =
                static_cast<double>(sound->sample_rate.load(std::memory_order_relaxed)) /
                static_cast<double>(output_rate);
            slot.position = 0.0;
            slot.position_frames.store(0, std::memory_order_release);
            slot.state.store(kVoiceSlotPlaying, std::memory_order_release);
            stat_voices_started.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        case kCommandPause: {
            VoiceSlot *slot = nullptr;
            if (!resolve_current_voice({command.voice_index, command.generation}, &slot)) {
                return;
            }
            if (slot->state.load(std::memory_order_acquire) == kVoiceSlotPlaying) {
                slot->state.store(kVoiceSlotPaused, std::memory_order_release);
            }
            return;
        }
        case kCommandResume: {
            VoiceSlot *slot = nullptr;
            if (!resolve_current_voice({command.voice_index, command.generation}, &slot)) {
                return;
            }
            if (slot->state.load(std::memory_order_acquire) == kVoiceSlotPaused) {
                slot->state.store(kVoiceSlotPlaying, std::memory_order_release);
            }
            return;
        }
        case kCommandStop: {
            VoiceSlot *slot = nullptr;
            if (!resolve_current_voice({command.voice_index, command.generation}, &slot)) {
                return;
            }
            uint32_t state = slot->state.load(std::memory_order_acquire);
            if (state == kVoiceSlotPlaying || state == kVoiceSlotPaused ||
                state == kVoiceSlotReserved) {
                slot->position = 0.0;
                slot->position_frames.store(0, std::memory_order_release);
                slot->state.store(kVoiceSlotFinished, std::memory_order_release);
                if (state != kVoiceSlotReserved) {
                    stat_voices_finished.fetch_add(1, std::memory_order_relaxed);
                }
            }
            return;
        }
        case kCommandSetVolume: {
            VoiceSlot *slot = nullptr;
            if (!resolve_current_voice({command.voice_index, command.generation}, &slot)) {
                return;
            }
            if (slot->state.load(std::memory_order_acquire) != kVoiceSlotFree) {
                slot->volume.store(clamp01(command.volume), std::memory_order_release);
            }
            return;
        }
        case kCommandSetPitch: {
            VoiceSlot *slot = nullptr;
            if (!resolve_current_voice({command.voice_index, command.generation}, &slot)) {
                return;
            }
            if (slot->state.load(std::memory_order_acquire) != kVoiceSlotFree) {
                slot->pitch.store(clamp_pitch(command.pitch), std::memory_order_release);
            }
            return;
        }
        case kCommandRemoveSound: {
            SoundSlot *sound = nullptr;
            const mikan_audio_sound handle{command.sound_index, command.sound_generation};
            if (!resolve_sound(handle, &sound)) {
                return;
            }
            for (uint32_t v = 0; v < MIKAN_AUDIO_MAX_VOICES; ++v) {
                VoiceSlot &slot = voices[v];
                if (slot.sound_index.load(std::memory_order_relaxed) != command.sound_index ||
                    slot.sound_generation.load(std::memory_order_relaxed) !=
                        command.sound_generation) {
                    continue;
                }
                uint32_t state = slot.state.load(std::memory_order_acquire);
                if (state == kVoiceSlotReserved || state == kVoiceSlotPlaying ||
                    state == kVoiceSlotPaused) {
                    slot.state.store(kVoiceSlotFinished, std::memory_order_release);
                    if (state != kVoiceSlotReserved) {
                        stat_voices_finished.fetch_add(1, std::memory_order_relaxed);
                        stat_orphaned.fetch_add(1, std::memory_order_relaxed);
                    }
                }
            }
            sound->state.store(kSoundDead, std::memory_order_release);
            stat_sounds_retired.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        default:
            return;
        }
    }

    void mix_voice(VoiceSlot &slot, float *frames, uint32_t frame_count, const float *bus_gain,
                   float master_gain) {
        const uint32_t bus = slot.bus.load(std::memory_order_relaxed);
        if (bus >= MIKAN_AUDIO_BUS_COUNT) {
            return;
        }
        const float gain =
            clamp01(slot.volume.load(std::memory_order_relaxed)) * bus_gain[bus] * master_gain;
        const float pitch = clamp_pitch(slot.pitch.load(std::memory_order_relaxed));
        const bool loop = slot.loop.load(std::memory_order_relaxed) != 0;
        const float *samples = slot.samples;
        const uint32_t length = slot.frames;
        const uint32_t channels = slot.channels;
        const double step = slot.base_ratio * static_cast<double>(pitch);
        double position = slot.position;
        uint32_t i = 0;

        for (; i < frame_count; ++i) {
            if (position >= static_cast<double>(length)) {
                if (!loop) {
                    break;
                }
                position = std::fmod(position, static_cast<double>(length));
            }
            const uint32_t first = static_cast<uint32_t>(position);
            const float fraction = static_cast<float>(position - static_cast<double>(first));
            float left0;
            float right0;
            float left1;
            float right1;
            if (channels == 2) {
                left0 = samples[first * 2];
                right0 = samples[first * 2 + 1];
                if (first + 1 < length) {
                    left1 = samples[(first + 1) * 2];
                    right1 = samples[(first + 1) * 2 + 1];
                } else if (loop) {
                    left1 = samples[0];
                    right1 = samples[1];
                } else {
                    /* The last frame interpolates toward silence: a clean
                     * one-frame fade instead of a click. */
                    left1 = 0.0f;
                    right1 = 0.0f;
                }
            } else {
                left0 = samples[first];
                right0 = left0;
                if (first + 1 < length) {
                    left1 = samples[first + 1];
                } else if (loop) {
                    left1 = samples[0];
                } else {
                    left1 = 0.0f;
                }
                right1 = left1;
            }
            frames[i * 2] += (left0 + (left1 - left0) * fraction) * gain;
            frames[i * 2 + 1] += (right0 + (right1 - right0) * fraction) * gain;
            position += step;
        }

        slot.position = position;
        if (!loop && position >= static_cast<double>(length)) {
            slot.position_frames.store(static_cast<uint64_t>(length), std::memory_order_release);
            slot.state.store(kVoiceSlotFinished, std::memory_order_release);
            stat_voices_finished.fetch_add(1, std::memory_order_relaxed);
        } else {
            slot.position_frames.store(static_cast<uint64_t>(position), std::memory_order_release);
        }
    }

    void render(float *frames, uint32_t frame_count) {
        if (frames == nullptr || frame_count == 0 || frame_count > kMaxRenderFrames) {
            return;
        }

        Command command;
        uint64_t applied = 0;
        while (queue.dequeue(&command)) {
            apply(command);
            applied += 1;
        }
        if (applied != 0) {
            stat_commands.fetch_add(applied, std::memory_order_relaxed);
        }

        float bus_gain[MIKAN_AUDIO_BUS_COUNT];
        for (uint32_t bus = 0; bus < MIKAN_AUDIO_BUS_COUNT; ++bus) {
            bus_gain[bus] = clamp01(bus_volume[bus].load(std::memory_order_relaxed));
        }
        const float master_gain = clamp01(master_volume.load(std::memory_order_relaxed));

        const uint32_t samples = frame_count * MIKAN_AUDIO_CHANNELS;
        for (uint32_t i = 0; i < samples; ++i) {
            frames[i] = 0.0f;
        }

        for (uint32_t v = 0; v < MIKAN_AUDIO_MAX_VOICES; ++v) {
            VoiceSlot &slot = voices[v];
            if (slot.state.load(std::memory_order_acquire) != kVoiceSlotPlaying) {
                continue;
            }
            mix_voice(slot, frames, frame_count, bus_gain, master_gain);
        }

        for (uint32_t i = 0; i < samples; ++i) {
            float value = frames[i];
            if (value > 1.0f) {
                value = 1.0f;
            } else if (value < -1.0f) {
                value = -1.0f;
            } else if (!(value == value)) { /* NaN from any source is silence */
                value = 0.0f;
            }
            frames[i] = value;
        }

        stat_blocks.fetch_add(1, std::memory_order_relaxed);
    }
};

/* ------------------------------------------------------------------ */
/* The control API                                                    */
/* ------------------------------------------------------------------ */

extern "C" int mikan_audio_runtime_create(uint32_t sample_rate,
                                          mikan_audio_runtime **out_runtime) {
    if (out_runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_runtime = nullptr;
    if (sample_rate < 8000u || sample_rate > 192000u) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    void *memory = malloc(sizeof(mikan_audio_runtime));
    if (memory == nullptr) {
        return MIKAN_AUDIO_OUT_OF_MEMORY;
    }
    *out_runtime = new (memory) mikan_audio_runtime(sample_rate);
    return MIKAN_AUDIO_OK;
}

extern "C" void mikan_audio_runtime_destroy(mikan_audio_runtime *runtime) {
    if (runtime == nullptr) {
        return;
    }
    runtime->~mikan_audio_runtime();
    free(runtime);
}

extern "C" int mikan_audio_runtime_add_sound(mikan_audio_runtime *runtime, const float *samples,
                                             uint32_t frame_count, uint32_t channels,
                                             uint32_t sample_rate, mikan_audio_sound *out_sound) {
    if (runtime == nullptr || samples == nullptr || out_sound == nullptr || frame_count == 0) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    out_sound->index = 0;
    out_sound->generation = 0;
    if (channels != 1 && channels != 2) {
        return MIKAN_AUDIO_UNSUPPORTED;
    }
    if (sample_rate < 8000u || sample_rate > 192000u) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (frame_count > (1u << 28)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }

    runtime->collect();

    uint32_t index = MIKAN_AUDIO_MAX_SOUNDS;
    for (uint32_t i = 0; i < MIKAN_AUDIO_MAX_SOUNDS; ++i) {
        if (runtime->sounds[i].state.load(std::memory_order_acquire) == kSoundFree) {
            index = i;
            break;
        }
    }
    if (index == MIKAN_AUDIO_MAX_SOUNDS) {
        return MIKAN_AUDIO_CAPACITY;
    }

    size_t count = static_cast<size_t>(frame_count) * channels;
    float *copy = static_cast<float *>(malloc(count * sizeof(float)));
    if (copy == nullptr) {
        return MIKAN_AUDIO_OUT_OF_MEMORY;
    }
    std::memcpy(copy, samples, count * sizeof(float));

    SoundSlot &slot = runtime->sounds[index];
    uint32_t generation = slot.generation.fetch_add(1, std::memory_order_acq_rel) + 1;
    slot.samples.store(copy, std::memory_order_relaxed);
    slot.frames.store(frame_count, std::memory_order_relaxed);
    slot.channels.store(channels, std::memory_order_relaxed);
    slot.sample_rate.store(sample_rate, std::memory_order_relaxed);
    slot.state.store(kSoundActive, std::memory_order_release);
    out_sound->index = index;
    out_sound->generation = generation;
    return MIKAN_AUDIO_OK;
}

extern "C" int mikan_audio_runtime_remove_sound(mikan_audio_runtime *runtime,
                                                mikan_audio_sound sound) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    SoundSlot *slot = nullptr;
    if (!runtime->resolve_sound(sound, &slot)) {
        return MIKAN_AUDIO_NOT_FOUND;
    }
    uint32_t state = slot->state.load(std::memory_order_acquire);
    if (state == kSoundFree) {
        return MIKAN_AUDIO_NOT_FOUND; /* removed and collected already */
    }
    if (state == kSoundRetired || state == kSoundDead) {
        return MIKAN_AUDIO_OK; /* idempotent */
    }
    /* Check space before publishing RETIRED: if the ack could not be
     * queued, the slot would never reach DEAD. */
    if (!runtime->queue.has_space()) {
        runtime->stat_rejected.fetch_add(1, std::memory_order_relaxed);
        return MIKAN_AUDIO_BUSY;
    }
    slot->state.store(kSoundRetired, std::memory_order_release);
    Command command;
    std::memset(&command, 0, sizeof(command));
    command.kind = kCommandRemoveSound;
    command.sound_index = sound.index;
    command.sound_generation = sound.generation;
    (void)runtime->queue.enqueue(command); /* has_space was checked */
    return MIKAN_AUDIO_OK;
}

extern "C" void mikan_audio_runtime_collect(mikan_audio_runtime *runtime) {
    if (runtime == nullptr) {
        return;
    }
    runtime->collect();
}

extern "C" int mikan_audio_runtime_sound_live(const mikan_audio_runtime *runtime,
                                              mikan_audio_sound sound, uint32_t *out_live) {
    if (runtime == nullptr || out_live == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    *out_live = 0;
    SoundSlot *slot = nullptr;
    if (!runtime->resolve_sound(sound, &slot)) {
        return MIKAN_AUDIO_OK;
    }
    *out_live = slot->state.load(std::memory_order_acquire) == kSoundActive ? 1u : 0u;
    return MIKAN_AUDIO_OK;
}

extern "C" int mikan_audio_runtime_play(mikan_audio_runtime *runtime,
                                        const mikan_audio_play_desc *desc,
                                        mikan_audio_voice *out_voice) {
    if (runtime == nullptr || desc == nullptr || out_voice == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (desc->bus >= MIKAN_AUDIO_BUS_COUNT) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(desc->volume)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(desc->pitch) || desc->pitch <= 0.0f) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    SoundSlot *sound = nullptr;
    if (!runtime->resolve_sound(desc->sound, &sound) ||
        sound->state.load(std::memory_order_acquire) != kSoundActive) {
        return MIKAN_AUDIO_NOT_FOUND;
    }

    uint32_t voice_index = MIKAN_AUDIO_MAX_VOICES;
    for (uint32_t v = 0; v < MIKAN_AUDIO_MAX_VOICES; ++v) {
        uint32_t state = runtime->voices[v].state.load(std::memory_order_acquire);
        if (state != kVoiceSlotFree && state != kVoiceSlotFinished) {
            continue;
        }
        uint32_t expected = state;
        if (runtime->voices[v].state.compare_exchange_strong(expected, kVoiceSlotReserved,
                                                             std::memory_order_acq_rel)) {
            voice_index = v;
            break;
        }
    }
    if (voice_index == MIKAN_AUDIO_MAX_VOICES) {
        return MIKAN_AUDIO_CAPACITY;
    }

    VoiceSlot &slot = runtime->voices[voice_index];
    uint32_t generation = slot.generation.fetch_add(1, std::memory_order_acq_rel) + 1;
    slot.sound_index.store(desc->sound.index, std::memory_order_relaxed);
    slot.sound_generation.store(desc->sound.generation, std::memory_order_relaxed);
    slot.bus.store(desc->bus, std::memory_order_relaxed);
    slot.loop.store(desc->loop != 0 ? 1u : 0u, std::memory_order_relaxed);
    slot.volume.store(clamp01(desc->volume), std::memory_order_relaxed);
    slot.pitch.store(clamp_pitch(desc->pitch), std::memory_order_relaxed);
    slot.position_frames.store(0, std::memory_order_relaxed);

    Command command;
    std::memset(&command, 0, sizeof(command));
    command.kind = kCommandPlay;
    command.voice_index = voice_index;
    command.generation = generation;
    command.sound_index = desc->sound.index;
    command.sound_generation = desc->sound.generation;
    if (!runtime->queue.enqueue(command)) {
        slot.state.store(kVoiceSlotFree, std::memory_order_release);
        runtime->stat_rejected.fetch_add(1, std::memory_order_relaxed);
        return MIKAN_AUDIO_BUSY;
    }

    out_voice->index = voice_index;
    out_voice->generation = generation;
    return MIKAN_AUDIO_OK;
}

/* The shared shape of pause/resume/stop/set: validate the handle and the
 * argument, then queue one command. Returns OK when the command is queued;
 * the render side ignores it if the voice moved on in the meantime. */
static int mikan_audio__queue_voice_command(mikan_audio_runtime *runtime,
                                            mikan_audio_voice voice, uint32_t kind, float value) {
    VoiceSlot *slot = nullptr;
    if (!runtime->resolve_current_voice(voice, &slot)) {
        return MIKAN_AUDIO_NOT_FOUND;
    }
    uint32_t state = slot->state.load(std::memory_order_acquire);
    if (state == kVoiceSlotFree) {
        return MIKAN_AUDIO_NOT_FOUND;
    }
    if (kind == kCommandStop) {
        if (state == kVoiceSlotFinished) {
            return MIKAN_AUDIO_NOT_FOUND; /* nothing live to stop */
        }
    } else if (kind == kCommandPause || kind == kCommandResume) {
        if (state == kVoiceSlotFinished) {
            return MIKAN_AUDIO_NOT_FOUND;
        }
    }
    if (!runtime->queue.has_space()) {
        runtime->stat_rejected.fetch_add(1, std::memory_order_relaxed);
        return MIKAN_AUDIO_BUSY;
    }
    Command command;
    std::memset(&command, 0, sizeof(command));
    command.kind = kind;
    command.voice_index = voice.index;
    command.generation = voice.generation;
    command.volume = value;
    command.pitch = value;
    (void)runtime->queue.enqueue(command); /* has_space was checked */
    return MIKAN_AUDIO_OK;
}

extern "C" int mikan_audio_runtime_pause(mikan_audio_runtime *runtime, mikan_audio_voice voice) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    return mikan_audio__queue_voice_command(runtime, voice, kCommandPause, 0.0f);
}

extern "C" int mikan_audio_runtime_resume(mikan_audio_runtime *runtime, mikan_audio_voice voice) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    return mikan_audio__queue_voice_command(runtime, voice, kCommandResume, 0.0f);
}

extern "C" int mikan_audio_runtime_stop(mikan_audio_runtime *runtime, mikan_audio_voice voice) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    return mikan_audio__queue_voice_command(runtime, voice, kCommandStop, 0.0f);
}

extern "C" int mikan_audio_runtime_set_voice_volume(mikan_audio_runtime *runtime,
                                                    mikan_audio_voice voice, float volume) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(volume)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    return mikan_audio__queue_voice_command(runtime, voice, kCommandSetVolume, volume);
}

extern "C" int mikan_audio_runtime_set_voice_pitch(mikan_audio_runtime *runtime,
                                                   mikan_audio_voice voice, float pitch) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(pitch) || pitch <= 0.0f) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    return mikan_audio__queue_voice_command(runtime, voice, kCommandSetPitch, pitch);
}

extern "C" int mikan_audio_runtime_set_bus_volume(mikan_audio_runtime *runtime, uint32_t bus,
                                                  float volume) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (bus >= MIKAN_AUDIO_BUS_COUNT) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(volume)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    runtime->bus_volume[bus].store(clamp01(volume), std::memory_order_release);
    return MIKAN_AUDIO_OK;
}

extern "C" int mikan_audio_runtime_set_master_volume(mikan_audio_runtime *runtime, float volume) {
    if (runtime == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    if (!std::isfinite(volume)) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    runtime->master_volume.store(clamp01(volume), std::memory_order_release);
    return MIKAN_AUDIO_OK;
}

/* ------------------------------------------------------------------ */
/* Queries                                                            */
/* ------------------------------------------------------------------ */

extern "C" int mikan_audio_runtime_voice_state(const mikan_audio_runtime *runtime,
                                               mikan_audio_voice voice,
                                               mikan_audio_voice_state *out_state) {
    if (runtime == nullptr || out_state == nullptr) {
        return MIKAN_AUDIO_BAD_ARGUMENT;
    }
    std::memset(out_state, 0, sizeof(*out_state));
    if (voice.index >= MIKAN_AUDIO_MAX_VOICES) {
        return MIKAN_AUDIO_NOT_FOUND;
    }
    const VoiceSlot &slot = runtime->voices[voice.index];
    uint32_t generation = slot.generation.load(std::memory_order_acquire);
    if (voice.generation == 0 || voice.generation > generation) {
        return MIKAN_AUDIO_NOT_FOUND;
    }
    if (voice.generation != generation) {
        /* Stale: the slot was recycled; the voice this handle named is
         * over, and its payload is deliberately zeroed rather than mixed
         * with its successor's. */
        out_state->state = MIKAN_AUDIO_VOICE_FINISHED;
        return MIKAN_AUDIO_OK;
    }
    uint32_t state = slot.state.load(std::memory_order_acquire);
    switch (state) {
    case kVoiceSlotReserved:
    case kVoiceSlotPlaying:
        out_state->state = MIKAN_AUDIO_VOICE_PLAYING;
        break;
    case kVoiceSlotPaused:
        out_state->state = MIKAN_AUDIO_VOICE_PAUSED;
        break;
    default:
        out_state->state = MIKAN_AUDIO_VOICE_FINISHED;
        break;
    }
    out_state->sound.index = slot.sound_index.load(std::memory_order_relaxed);
    out_state->sound.generation = slot.sound_generation.load(std::memory_order_relaxed);
    out_state->loop = slot.loop.load(std::memory_order_relaxed);
    out_state->volume = slot.volume.load(std::memory_order_relaxed);
    out_state->pitch = slot.pitch.load(std::memory_order_relaxed);
    out_state->position_frames = slot.position_frames.load(std::memory_order_acquire);
    return MIKAN_AUDIO_OK;
}

extern "C" void mikan_audio_runtime_stats_into(const mikan_audio_runtime *runtime,
                                               mikan_audio_runtime_stats *out_stats) {
    if (runtime == nullptr || out_stats == nullptr) {
        return;
    }
    out_stats->voices_started =
        runtime->stat_voices_started.load(std::memory_order_relaxed);
    out_stats->voices_finished =
        runtime->stat_voices_finished.load(std::memory_order_relaxed);
    out_stats->blocks_mixed = runtime->stat_blocks.load(std::memory_order_relaxed);
    out_stats->commands_applied = runtime->stat_commands.load(std::memory_order_relaxed);
    out_stats->commands_rejected = runtime->stat_rejected.load(std::memory_order_relaxed);
    out_stats->sounds_retired = runtime->stat_sounds_retired.load(std::memory_order_relaxed);
    out_stats->voices_orphaned = runtime->stat_orphaned.load(std::memory_order_relaxed);
}

extern "C" void mikan_audio_runtime_render(void *user, float *frames, uint32_t frame_count) {
    if (user == nullptr) {
        return;
    }
    static_cast<mikan_audio_runtime *>(user)->render(frames, frame_count);
}
