/*
 * Mikan input runtime — the C++ action layer of the input system.
 *
 * Consumes the normalized device stream the C backend produces and answers
 * the gameplay questions: which logical actions are held, which edges fired
 * this step, and with what analog value. Actions are grouped into contexts
 * with explicit priorities; an input is consumed by the highest-priority
 * active context that binds it, so overlapping contexts have deterministic,
 * documented precedence instead of hidden global state.
 *
 * The runtime owns action state only: raw device state belongs to the
 * backend, cursor position to the platform layer, and nothing here mirrors
 * either. See mikan_input.h for the contract.
 */

#include "mikan_input.h"

#include <cstddef>
#include <new>
#include <unordered_map>
#include <vector>

namespace {

/* A binding is keyed by its device input inside one context. */
uint64_t input_key(uint32_t device, uint32_t code) {
    return (static_cast<uint64_t>(device) << 32) | static_cast<uint64_t>(code);
}

struct binding {
    uint32_t action;
    bool down;
};

struct input_context {
    uint32_t id;
    uint32_t priority;
    bool active;
    std::unordered_map<uint64_t, binding> bindings;
};

struct action_state {
    size_t held_count;
    bool just_pressed;
    bool just_released;
    bool repeating;
    /* The step's wheel contribution; folded into `value` and cleared by
     * end_step. Tracked apart so the digital part stays exact. */
    float wheel;
    /* The analog value: 1.0 while any bound input is held, plus the
     * step's wheel deltas. Maintained incrementally and recomputed
     * exactly at end_step. */
    float value;
};

/* The idle state every unbound action reports. */
const action_state IDLE = {0, false, false, false, 0.0f, 0.0f};

}  // namespace

struct mikan_input_runtime {
    std::vector<input_context> contexts;
    std::unordered_map<uint32_t, action_state> actions;
    uint32_t next_context_id;
};

namespace {

input_context *find_context(mikan_input_runtime *runtime, uint32_t id) {
    for (size_t i = 0; i < runtime->contexts.size(); ++i) {
        if (runtime->contexts[i].id == id) {
            return &runtime->contexts[i];
        }
    }
    return NULL;
}

const input_context *find_context(const mikan_input_runtime *runtime, uint32_t id) {
    for (size_t i = 0; i < runtime->contexts.size(); ++i) {
        if (runtime->contexts[i].id == id) {
            return &runtime->contexts[i];
        }
    }
    return NULL;
}

binding *find_binding(mikan_input_runtime *runtime, uint32_t device,
                      uint32_t code) {
    /* Precedence: the highest-priority active context that binds the input
     * consumes it; ties prefer the context created later (larger id). */
    const uint64_t key = input_key(device, code);
    input_context *best = NULL;
    for (size_t i = 0; i < runtime->contexts.size(); ++i) {
        input_context &candidate = runtime->contexts[i];
        if (!candidate.active
            || candidate.bindings.find(key) == candidate.bindings.end()) {
            continue;
        }
        if (best == NULL || candidate.priority > best->priority
            || (candidate.priority == best->priority
                && candidate.id > best->id)) {
            best = &candidate;
        }
    }
    return best == NULL ? NULL : &best->bindings[key];
}

action_state &action_of(mikan_input_runtime *runtime, uint32_t action) {
    return runtime->actions[action];
}

/* Releases one held binding: its action loses a holder and gains a
 * just-released edge when the last holder goes. */
void release_binding(mikan_input_runtime *runtime, binding &held) {
    if (!held.down) {
        return;
    }
    held.down = false;
    action_state &state = action_of(runtime, held.action);
    if (state.held_count > 0) {
        state.held_count -= 1;
        if (state.held_count == 0) {
            state.just_released = true;
            state.repeating = false;
            /* The digital part of the value goes with its last holder. */
            state.value -= 1.0f;
        }
    }
}

void release_held_bindings(mikan_input_runtime *runtime, uint32_t device,
                           uint32_t code) {
    const uint64_t key = input_key(device, code);
    for (size_t i = 0; i < runtime->contexts.size(); ++i) {
        std::unordered_map<uint64_t, binding>::iterator it =
            runtime->contexts[i].bindings.find(key);
        if (it != runtime->contexts[i].bindings.end()) {
            release_binding(runtime, it->second);
        }
    }
}

}  // namespace

mikan_input_runtime *mikan_input_runtime_create(void) {
    mikan_input_runtime *runtime = new (std::nothrow) mikan_input_runtime();
    if (runtime == NULL) {
        return NULL;
    }
    runtime->next_context_id = 1;
    return runtime;
}

void mikan_input_runtime_destroy(mikan_input_runtime *runtime) {
    delete runtime;
}

int mikan_input_runtime_add_context(mikan_input_runtime *runtime,
                                    uint32_t priority, uint32_t *out_context) {
    if (runtime == NULL || out_context == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    /* Allocation failure is fatal by contract (the module builds without
     * exception support), like every other allocation here. */
    input_context added;
    added.id = runtime->next_context_id;
    added.priority = priority;
    added.active = true;
    runtime->contexts.push_back(added);
    *out_context = added.id;
    runtime->next_context_id += 1;
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_set_context_active(mikan_input_runtime *runtime,
                                           uint32_t context, int active) {
    if (runtime == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    input_context *found = find_context(runtime, context);
    if (found == NULL) {
        return MIKAN_INPUT_UNKNOWN_CONTEXT;
    }
    found->active = active != 0;
    if (!found->active) {
        /* Deactivation releases everything the context held, so an action
         * cannot stay held by a context that no longer receives input. */
        for (std::unordered_map<uint64_t, binding>::iterator it =
                 found->bindings.begin();
             it != found->bindings.end(); ++it) {
            release_binding(runtime, it->second);
        }
    }
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_remove_context(mikan_input_runtime *runtime,
                                       uint32_t context) {
    if (runtime == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    for (size_t i = 0; i < runtime->contexts.size(); ++i) {
        if (runtime->contexts[i].id == context) {
            if (runtime->contexts[i].active) {
                return MIKAN_INPUT_BAD_ARGUMENT; /* remove the inactive only */
            }
            runtime->contexts.erase(runtime->contexts.begin()
                                    + static_cast<ptrdiff_t>(i));
            return MIKAN_INPUT_OK;
        }
    }
    return MIKAN_INPUT_UNKNOWN_CONTEXT;
}

int mikan_input_runtime_bind(mikan_input_runtime *runtime, uint32_t context,
                             uint32_t device, uint32_t code, uint32_t action) {
    if (runtime == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    input_context *owner = find_context(runtime, context);
    if (owner == NULL) {
        return MIKAN_INPUT_UNKNOWN_CONTEXT;
    }
    /* Wheel axes are codes 0 and 1 and are the one device kind where code
     * 0 is meaningful; keys and buttons reserve 0 and reject it. */
    if (device == MIKAN_INPUT_EVENT_WHEEL) {
        if (code != MIKAN_INPUT_WHEEL_HORIZONTAL
            && code != MIKAN_INPUT_WHEEL_VERTICAL) {
            return MIKAN_INPUT_BAD_ARGUMENT;
        }
    } else if (device == MIKAN_INPUT_EVENT_KEY_PRESS
               || device == MIKAN_INPUT_EVENT_MOUSE_PRESS) {
        if (code == 0) {
            return MIKAN_INPUT_BAD_ARGUMENT;
        }
    } else {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    const uint64_t key = input_key(device, code);
    if (owner->bindings.find(key) != owner->bindings.end()) {
        return MIKAN_INPUT_CONFLICT;
    }
    binding added;
    added.action = action;
    added.down = false;
    owner->bindings[key] = added;
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_unbind(mikan_input_runtime *runtime, uint32_t context,
                               uint32_t device, uint32_t code) {
    if (runtime == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    input_context *owner = find_context(runtime, context);
    if (owner == NULL) {
        return MIKAN_INPUT_UNKNOWN_CONTEXT;
    }
    std::unordered_map<uint64_t, binding>::iterator it =
        owner->bindings.find(input_key(device, code));
    if (it == owner->bindings.end()) {
        return MIKAN_INPUT_NOT_FOUND;
    }
    release_binding(runtime, it->second);
    owner->bindings.erase(it);
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_binding_count(const mikan_input_runtime *runtime,
                                      uint32_t context, size_t *out_count) {
    if (runtime == NULL || out_count == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    const input_context *owner = find_context(runtime, context);
    if (owner == NULL) {
        return MIKAN_INPUT_UNKNOWN_CONTEXT;
    }
    *out_count = owner->bindings.size();
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_process(mikan_input_runtime *runtime,
                                const mikan_input_event *events,
                                size_t count) {
    if (runtime == NULL || (count > 0 && events == NULL)) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    for (size_t i = 0; i < count; ++i) {
        const mikan_input_event &event = events[i];
        switch (event.type) {
            case MIKAN_INPUT_EVENT_FOCUS_LOST: {
                /* Defense in depth: the backend already synthesized one
                 * release per held device. This sweep also covers any
                 * binding state that could not be matched to a release. */
                for (size_t c = 0; c < runtime->contexts.size(); ++c) {
                    std::unordered_map<uint64_t, binding> &bindings =
                        runtime->contexts[c].bindings;
                    for (std::unordered_map<uint64_t, binding>::iterator it =
                             bindings.begin();
                         it != bindings.end(); ++it) {
                        release_binding(runtime, it->second);
                    }
                }
                break;
            }
            case MIKAN_INPUT_EVENT_FOCUS_GAINED:
            case MIKAN_INPUT_EVENT_MOUSE_MOVE:
                break;
            case MIKAN_INPUT_EVENT_KEY_REPEAT: {
                binding *held = find_binding(
                    runtime, MIKAN_INPUT_EVENT_KEY_PRESS, event.code);
                if (held != NULL && held->down) {
                    action_of(runtime, held->action).repeating = true;
                }
                break;
            }
            case MIKAN_INPUT_EVENT_KEY_PRESS:
            case MIKAN_INPUT_EVENT_MOUSE_PRESS: {
                const uint32_t device =
                    event.type == MIKAN_INPUT_EVENT_KEY_PRESS
                        ? MIKAN_INPUT_EVENT_KEY_PRESS
                        : MIKAN_INPUT_EVENT_MOUSE_PRESS;
                binding *match = find_binding(runtime, device, event.code);
                if (match == NULL || match->down) {
                    break; /* unconsumed, or already down */
                }
                match->down = true;
                action_state &state = action_of(runtime, match->action);
                state.held_count += 1;
                if (state.held_count == 1) {
                    state.just_pressed = true;
                    state.value += 1.0f;
                }
                break;
            }
            case MIKAN_INPUT_EVENT_KEY_RELEASE:
            case MIKAN_INPUT_EVENT_MOUSE_RELEASE: {
                const uint32_t device =
                    event.type == MIKAN_INPUT_EVENT_KEY_RELEASE
                        ? MIKAN_INPUT_EVENT_KEY_PRESS
                        : MIKAN_INPUT_EVENT_MOUSE_PRESS;
                /* Release the context that consumed the press, even if a
                 * higher-priority context became active in the meantime. */
                release_held_bindings(runtime, device, event.code);
                break;
            }
            case MIKAN_INPUT_EVENT_WHEEL: {
                /* One wheel event carries both axis deltas; the axes are
                 * independent inputs, so each axis binding consumes its own
                 * delta (per axis, the usual priority consumption applies). */
                for (uint32_t axis = MIKAN_INPUT_WHEEL_HORIZONTAL;
                     axis <= MIKAN_INPUT_WHEEL_VERTICAL; ++axis) {
                    binding *match = find_binding(runtime, MIKAN_INPUT_EVENT_WHEEL, axis);
                    if (match == NULL) {
                        continue;
                    }
                    const double delta =
                        axis == MIKAN_INPUT_WHEEL_HORIZONTAL ? event.x : event.y;
                    if (delta == 0.0) {
                        continue;
                    }
                    action_state &state = action_of(runtime, match->action);
                    state.wheel += static_cast<float>(delta);
                    state.value += static_cast<float>(delta);
                }
                break;
            }
            default:
                break; /* unknown event kinds are ignored, not fatal */
        }
    }
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_end_step(mikan_input_runtime *runtime) {
    if (runtime == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    for (std::unordered_map<uint32_t, action_state>::iterator it =
             runtime->actions.begin();
         it != runtime->actions.end(); ++it) {
        it->second.just_pressed = false;
        it->second.just_released = false;
        it->second.repeating = false;
        it->second.wheel = 0.0f;
        /* The wheel contribution is per step; the digital part follows
         * held_count, recomputed here so no float drift can accumulate. */
        it->second.value = it->second.held_count > 0 ? 1.0f : 0.0f;
    }
    return MIKAN_INPUT_OK;
}

int mikan_input_runtime_action_state(const mikan_input_runtime *runtime,
                                     uint32_t action,
                                     mikan_input_action_state *out_state) {
    if (runtime == NULL || out_state == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    std::unordered_map<uint32_t, action_state>::const_iterator it =
        runtime->actions.find(action);
    const action_state &state = it == runtime->actions.end() ? IDLE : it->second;
    out_state->held = state.held_count > 0 ? 1 : 0;
    out_state->just_pressed = state.just_pressed ? 1 : 0;
    out_state->just_released = state.just_released ? 1 : 0;
    out_state->repeating = state.repeating ? 1 : 0;
    out_state->value = state.value;
    return MIKAN_INPUT_OK;
}
