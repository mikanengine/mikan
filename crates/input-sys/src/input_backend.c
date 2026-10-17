/*
 * Mikan input backend — the C device layer of the input system.
 *
 * Owns the canonical event buffer and the raw device state, and turns the
 * raw stream an application feeds it into a normalized one. See
 * mikan_input.h for the contract.
 */

#include "mikan_input.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

/* The default event-buffer capacity, selected by a zero capacity. */
#define MIKAN_INPUT_DEFAULT_CAPACITY 4096

struct mikan_input_backend {
    /* The bounded event buffer. Events append at `count`; readers drain
     * from the front and the remainder moves down. Overflow drops the
     * newest event and counts it. */
    mikan_input_event *events;
    size_t capacity;
    size_t count;
    size_t dropped;

    /* Raw device state: the codes currently held down. These arrays stay
     * small in practice (a few held keys, a few buttons), so linear
     * scans beat a hash table and keep the module allocation-light. */
    uint32_t *keys_down;
    size_t keys_count;
    size_t keys_capacity;

    uint32_t *buttons_down;
    size_t buttons_count;
    size_t buttons_capacity;

    /* The last focus state seen, so duplicate focus events are no-ops. */
    int focused;
};

static int append_event(mikan_input_backend *backend, uint32_t type,
                        uint32_t code, double x, double y) {
    if (backend->count == backend->capacity) {
        backend->dropped += 1;
        return MIKAN_INPUT_OVERFLOW;
    }
    backend->events[backend->count].type = type;
    backend->events[backend->count].code = code;
    backend->events[backend->count].x = x;
    backend->events[backend->count].y = y;
    backend->count += 1;
    return MIKAN_INPUT_OK;
}

/* Appends, coalescing into a trailing event of the same type. A trailing
 * MOUSE_MOVE is overwritten (only the latest position matters) and a
 * trailing WHEEL is summed (every scroll matters, but one summed event
 * carries the same total). `combine` selects the reduction. */
static int append_coalesced(mikan_input_backend *backend, uint32_t type,
                            int combine, double x, double y) {
    if (backend->count > 0) {
        mikan_input_event *last = &backend->events[backend->count - 1];
        if (last->type == type) {
            if (combine) {
                last->x += x;
                last->y += y;
            } else {
                last->x = x;
                last->y = y;
            }
            return MIKAN_INPUT_OK;
        }
    }
    return append_event(backend, type, 0, x, y);
}

static int is_down(const uint32_t *codes, size_t count, uint32_t code) {
    for (size_t i = 0; i < count; ++i) {
        if (codes[i] == code) {
            return 1;
        }
    }
    return 0;
}

/* Removes `code` from a held array, preserving the order of the rest so
 * focus-loss releases are emitted in the order the devices went down.
 * Held counts are small (a few keys and buttons), so the shift is cheap
 * and keeps the state deterministic, not an unordered set. */
static void remove_down(uint32_t *codes, size_t *count, uint32_t code) {
    for (size_t i = 0; i < *count; ++i) {
        if (codes[i] == code) {
            memmove(codes + i, codes + i + 1,
                    (*count - i - 1) * sizeof(*codes));
            *count -= 1;
            return;
        }
    }
}

static int add_down(uint32_t **codes, size_t *count, size_t *capacity,
                    uint32_t code) {
    if (*count == *capacity) {
        size_t grown = *capacity == 0 ? 8 : *capacity * 2;
        uint32_t *retry = realloc(*codes, grown * sizeof(**codes));
        if (retry == NULL) {
            return 0;
        }
        *codes = retry;
        *capacity = grown;
    }
    (*codes)[*count] = code;
    *count += 1;
    return 1;
}

/* The shared press/release normalization for keys and buttons. */
static int push_transition(mikan_input_backend *backend, uint32_t press_type,
                           uint32_t release_type, uint32_t code, int pressed,
                           uint32_t **down, size_t *down_count,
                           size_t *down_capacity) {
    if (pressed) {
        if (is_down(*down, *down_count, code)) {
            return MIKAN_INPUT_OK; /* duplicate press: dropped */
        }
        if (!add_down(down, down_count, down_capacity, code)) {
            return MIKAN_INPUT_BAD_ARGUMENT; /* allocation failure */
        }
        return append_event(backend, press_type, code, 0.0, 0.0);
    }
    if (!is_down(*down, *down_count, code)) {
        return MIKAN_INPUT_OK; /* release of an up device: dropped */
    }
    remove_down(*down, down_count, code);
    return append_event(backend, release_type, code, 0.0, 0.0);
}

mikan_input_backend *mikan_input_backend_create(size_t capacity) {
    if (capacity == 0) {
        capacity = MIKAN_INPUT_DEFAULT_CAPACITY;
    }
    mikan_input_backend *backend = calloc(1, sizeof(*backend));
    if (backend == NULL) {
        return NULL;
    }
    backend->events = calloc(capacity, sizeof(*backend->events));
    if (backend->events == NULL) {
        free(backend);
        return NULL;
    }
    backend->capacity = capacity;
    return backend;
}

void mikan_input_backend_destroy(mikan_input_backend *backend) {
    if (backend == NULL) {
        return;
    }
    free(backend->events);
    free(backend->keys_down);
    free(backend->buttons_down);
    free(backend);
}

int mikan_input_backend_push_key(mikan_input_backend *backend, uint32_t code,
                                 int pressed, int repeat) {
    if (backend == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    if (pressed && repeat) {
        /* An operating-system repeat only counts while the key is down;
         * the state table is the truth, not the stream. */
        if (!is_down(backend->keys_down, backend->keys_count, code)) {
            return MIKAN_INPUT_OK;
        }
        return append_event(backend, MIKAN_INPUT_EVENT_KEY_REPEAT, code, 0.0,
                            0.0);
    }
    return push_transition(backend, MIKAN_INPUT_EVENT_KEY_PRESS,
                           MIKAN_INPUT_EVENT_KEY_RELEASE, code, pressed,
                           &backend->keys_down, &backend->keys_count,
                           &backend->keys_capacity);
}

int mikan_input_backend_push_mouse_button(mikan_input_backend *backend,
                                          uint32_t code, int pressed) {
    if (backend == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    return push_transition(backend, MIKAN_INPUT_EVENT_MOUSE_PRESS,
                           MIKAN_INPUT_EVENT_MOUSE_RELEASE, code, pressed,
                           &backend->buttons_down, &backend->buttons_count,
                           &backend->buttons_capacity);
}

int mikan_input_backend_push_mouse_move(mikan_input_backend *backend, double x,
                                        double y) {
    if (backend == NULL || !isfinite(x) || !isfinite(y)) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    return append_coalesced(backend, MIKAN_INPUT_EVENT_MOUSE_MOVE, 0, x, y);
}

int mikan_input_backend_push_wheel(mikan_input_backend *backend, double dx,
                                   double dy) {
    if (backend == NULL || !isfinite(dx) || !isfinite(dy)) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    if (dx == 0.0 && dy == 0.0) {
        return MIKAN_INPUT_OK; /* a zero scroll is not an event */
    }
    return append_coalesced(backend, MIKAN_INPUT_EVENT_WHEEL, 1, dx, dy);
}

int mikan_input_backend_push_focus(mikan_input_backend *backend, int focused) {
    if (backend == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    if ((backend->focused != 0) == (focused != 0)) {
        return MIKAN_INPUT_OK; /* unchanged focus: nothing to do */
    }
    backend->focused = focused;
    if (focused) {
        return append_event(backend, MIKAN_INPUT_EVENT_FOCUS_GAINED, 0, 0.0,
                            0.0);
    }
    /* Losing focus releases every held device first, in device order, so
     * the runtime sees ordinary releases and cannot keep an action held
     * across a focus change; FOCUS_LOST then sweeps whatever is left. */
    int status = MIKAN_INPUT_OK;
    for (size_t i = 0; i < backend->keys_count && status == MIKAN_INPUT_OK;
         ++i) {
        status = append_event(backend, MIKAN_INPUT_EVENT_KEY_RELEASE,
                              backend->keys_down[i], 0.0, 0.0);
    }
    for (size_t i = 0; i < backend->buttons_count && status == MIKAN_INPUT_OK;
         ++i) {
        status = append_event(backend, MIKAN_INPUT_EVENT_MOUSE_RELEASE,
                              backend->buttons_down[i], 0.0, 0.0);
    }
    if (status != MIKAN_INPUT_OK) {
        return status;
    }
    backend->keys_count = 0;
    backend->buttons_count = 0;
    return append_event(backend, MIKAN_INPUT_EVENT_FOCUS_LOST, 0, 0.0, 0.0);
}

int mikan_input_backend_is_key_down(const mikan_input_backend *backend,
                                    uint32_t code) {
    if (backend == NULL) {
        return 0;
    }
    return is_down(backend->keys_down, backend->keys_count, code);
}

int mikan_input_backend_is_mouse_button_down(const mikan_input_backend *backend,
                                             uint32_t code) {
    if (backend == NULL) {
        return 0;
    }
    return is_down(backend->buttons_down, backend->buttons_count, code);
}

size_t mikan_input_backend_len(const mikan_input_backend *backend) {
    return backend == NULL ? 0 : backend->count;
}

size_t mikan_input_backend_dropped(const mikan_input_backend *backend) {
    return backend == NULL ? 0 : backend->dropped;
}

int mikan_input_backend_read(mikan_input_backend *backend,
                             mikan_input_event *out_events, size_t capacity,
                             size_t *out_count) {
    if (out_count == NULL || (capacity > 0 && out_events == NULL)) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    if (backend == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    size_t copied = backend->count < capacity ? backend->count : capacity;
    memcpy(out_events, backend->events, copied * sizeof(*out_events));
    /* Drain what was copied: the remainder moves to the front and the
     * buffer stays compact, so the next append reuses the same array. */
    memmove(backend->events, backend->events + copied,
            (backend->count - copied) * sizeof(*backend->events));
    backend->count -= copied;
    *out_count = copied;
    return MIKAN_INPUT_OK;
}

int mikan_input_backend_clear(mikan_input_backend *backend) {
    if (backend == NULL) {
        return MIKAN_INPUT_BAD_ARGUMENT;
    }
    backend->count = 0;
    return MIKAN_INPUT_OK;
}
