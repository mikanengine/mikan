/*
 * Mikan input — the native boundary of the input system.
 *
 * This header is the contract. It is implemented twice, by design:
 *
 *   - input_backend.c   (C): the low-level device backend. It owns the
 *     canonical event model, a bounded event buffer, and the raw device
 *     state (which keys and mouse buttons are down). It normalizes the
 *     raw event stream: duplicate presses, releases of keys that are not
 *     down, and spurious repeats are suppressed; consecutive mouse moves
 *     and wheel scrolls are coalesced; losing focus releases every held
 *     device explicitly. Its output is a normalized event stream.
 *
 *   - input_runtime.cpp (C++): the action runtime. It consumes the
 *     normalized stream and answers gameplay questions: physical inputs
 *     are bound to logical actions, actions are grouped into contexts
 *     with explicit priorities and activation rules, and every action
 *     carries a state machine (held, just-pressed, just-released,
 *     repeating, analog value). It is authoritative for actions; the
 *     backend is authoritative for devices.
 *
 * Rust owns configuration, safe handles, frame integration, and policy
 * (crates/input). The two native layers never call each other: Rust
 * drains the backend's buffer and feeds the runtime in one batch per
 * fixed step, so the boundary crosses only plain data.
 *
 * Semantics shared by both layers:
 *
 *   - Every function validates its arguments and returns a status code;
 *     null pointers and unknown handles are MIKAN_INPUT_BAD_ARGUMENT,
 *     never a crash. Allocation failure is fatal (the module is built
 *     without exception support); nothing throws across this boundary.
 *   - Event codes are plain u32 values. Key codes are the MIKAN_KEY_*
 *     constants below (code 0 is reserved and unbindable); mouse buttons
 *     are MIKAN_MOUSE_LEFT/RIGHT/MIDDLE plus MIKAN_MOUSE_OTHER_BASE + n
 *     for numbered extra buttons.
 *   - The runtime resolves an input against active contexts in priority
 *     order: the highest-priority active context that binds the input
 *     consumes it; lower contexts never see it. Two bindings for the
 *     same input inside one context are rejected (MIKAN_INPUT_CONFLICT).
 *   - Losing focus leaves nothing stuck: the backend synthesizes one
 *     release per held device and a MIKAN_INPUT_EVENT_FOCUS_LOST, and
 *     the runtime additionally sweeps all binding state on FOCUS_LOST.
 *   - Not thread-safe: a backend or runtime instance requires exclusive
 *     access, exactly as its Rust wrapper does through &mut self.
 */
#ifndef MIKAN_INPUT_H
#define MIKAN_INPUT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---------------------------------------------------------------- events */

enum {
    MIKAN_INPUT_EVENT_KEY_PRESS = 1,
    MIKAN_INPUT_EVENT_KEY_RELEASE = 2,
    MIKAN_INPUT_EVENT_KEY_REPEAT = 3,
    MIKAN_INPUT_EVENT_MOUSE_PRESS = 4,
    MIKAN_INPUT_EVENT_MOUSE_RELEASE = 5,
    MIKAN_INPUT_EVENT_MOUSE_MOVE = 6,
    MIKAN_INPUT_EVENT_WHEEL = 7,
    MIKAN_INPUT_EVENT_FOCUS_LOST = 8,
    MIKAN_INPUT_EVENT_FOCUS_GAINED = 9
};

enum {
    MIKAN_INPUT_MOUSE_NONE = 0,
    MIKAN_INPUT_MOUSE_LEFT = 1,
    MIKAN_INPUT_MOUSE_RIGHT = 2,
    MIKAN_INPUT_MOUSE_MIDDLE = 3,
    MIKAN_INPUT_MOUSE_OTHER_BASE = 256
};

/* Key codes are USB HID keyboard usage IDs (usage page 1), a stable,
 * documented numbering that maps one-to-one onto the engine's key names.
 * Code 0 (MIKAN_INPUT_KEY_UNKNOWN) is reserved: it tracks raw key state
 * but can never be bound. The table covers the keys a desktop engine
 * binds in practice; the Rust layer maps its key names onto these values
 * and reports anything outside the table as 0. */
enum {
    MIKAN_INPUT_KEY_UNKNOWN = 0,
    MIKAN_INPUT_KEY_A = 0x04,
    MIKAN_INPUT_KEY_B = 0x05,
    MIKAN_INPUT_KEY_C = 0x06,
    MIKAN_INPUT_KEY_D = 0x07,
    MIKAN_INPUT_KEY_E = 0x08,
    MIKAN_INPUT_KEY_F = 0x09,
    MIKAN_INPUT_KEY_G = 0x0A,
    MIKAN_INPUT_KEY_H = 0x0B,
    MIKAN_INPUT_KEY_I = 0x0C,
    MIKAN_INPUT_KEY_J = 0x0D,
    MIKAN_INPUT_KEY_K = 0x0E,
    MIKAN_INPUT_KEY_L = 0x0F,
    MIKAN_INPUT_KEY_M = 0x10,
    MIKAN_INPUT_KEY_N = 0x11,
    MIKAN_INPUT_KEY_O = 0x12,
    MIKAN_INPUT_KEY_P = 0x13,
    MIKAN_INPUT_KEY_Q = 0x14,
    MIKAN_INPUT_KEY_R = 0x15,
    MIKAN_INPUT_KEY_S = 0x16,
    MIKAN_INPUT_KEY_T = 0x17,
    MIKAN_INPUT_KEY_U = 0x18,
    MIKAN_INPUT_KEY_V = 0x19,
    MIKAN_INPUT_KEY_W = 0x1A,
    MIKAN_INPUT_KEY_X = 0x1B,
    MIKAN_INPUT_KEY_Y = 0x1C,
    MIKAN_INPUT_KEY_Z = 0x1D,
    MIKAN_INPUT_KEY_DIGIT1 = 0x1E,
    MIKAN_INPUT_KEY_DIGIT2 = 0x1F,
    MIKAN_INPUT_KEY_DIGIT3 = 0x20,
    MIKAN_INPUT_KEY_DIGIT4 = 0x21,
    MIKAN_INPUT_KEY_DIGIT5 = 0x22,
    MIKAN_INPUT_KEY_DIGIT6 = 0x23,
    MIKAN_INPUT_KEY_DIGIT7 = 0x24,
    MIKAN_INPUT_KEY_DIGIT8 = 0x25,
    MIKAN_INPUT_KEY_DIGIT9 = 0x26,
    MIKAN_INPUT_KEY_DIGIT0 = 0x27,
    MIKAN_INPUT_KEY_ENTER = 0x28,
    MIKAN_INPUT_KEY_ESCAPE = 0x29,
    MIKAN_INPUT_KEY_BACKSPACE = 0x2A,
    MIKAN_INPUT_KEY_TAB = 0x2B,
    MIKAN_INPUT_KEY_SPACE = 0x2C,
    MIKAN_INPUT_KEY_MINUS = 0x2D,
    MIKAN_INPUT_KEY_EQUAL = 0x2E,
    MIKAN_INPUT_KEY_BRACKETLEFT = 0x2F,
    MIKAN_INPUT_KEY_BRACKETRIGHT = 0x30,
    MIKAN_INPUT_KEY_BACKSLASH = 0x31,
    MIKAN_INPUT_KEY_SEMICOLON = 0x33,
    MIKAN_INPUT_KEY_QUOTE = 0x34,
    MIKAN_INPUT_KEY_BACKQUOTE = 0x35,
    MIKAN_INPUT_KEY_COMMA = 0x36,
    MIKAN_INPUT_KEY_PERIOD = 0x37,
    MIKAN_INPUT_KEY_SLASH = 0x38,
    MIKAN_INPUT_KEY_CAPSLOCK = 0x39,
    MIKAN_INPUT_KEY_F1 = 0x3A,
    MIKAN_INPUT_KEY_F2 = 0x3B,
    MIKAN_INPUT_KEY_F3 = 0x3C,
    MIKAN_INPUT_KEY_F4 = 0x3D,
    MIKAN_INPUT_KEY_F5 = 0x3E,
    MIKAN_INPUT_KEY_F6 = 0x3F,
    MIKAN_INPUT_KEY_F7 = 0x40,
    MIKAN_INPUT_KEY_F8 = 0x41,
    MIKAN_INPUT_KEY_F9 = 0x42,
    MIKAN_INPUT_KEY_F10 = 0x43,
    MIKAN_INPUT_KEY_F11 = 0x44,
    MIKAN_INPUT_KEY_F12 = 0x45,
    MIKAN_INPUT_KEY_INSERT = 0x49,
    MIKAN_INPUT_KEY_HOME = 0x4A,
    MIKAN_INPUT_KEY_PAGEUP = 0x4B,
    MIKAN_INPUT_KEY_DELETE = 0x4C,
    MIKAN_INPUT_KEY_END = 0x4D,
    MIKAN_INPUT_KEY_PAGEDOWN = 0x4E,
    MIKAN_INPUT_KEY_ARROWRIGHT = 0x4F,
    MIKAN_INPUT_KEY_ARROWLEFT = 0x50,
    MIKAN_INPUT_KEY_ARROWDOWN = 0x51,
    MIKAN_INPUT_KEY_ARROWUP = 0x52,
    MIKAN_INPUT_KEY_CONTROLLEFT = 0xE0,
    MIKAN_INPUT_KEY_SHIFTLEFT = 0xE1,
    MIKAN_INPUT_KEY_ALTLEFT = 0xE2,
    MIKAN_INPUT_KEY_METALEFT = 0xE3,
    MIKAN_INPUT_KEY_CONTROLRIGHT = 0xE4,
    MIKAN_INPUT_KEY_SHIFTRIGHT = 0xE5,
    MIKAN_INPUT_KEY_ALTRIGHT = 0xE6,
    MIKAN_INPUT_KEY_METARIGHT = 0xE7
};

/* Wheel axes, used as the code of a wheel binding. */
enum {
    MIKAN_INPUT_WHEEL_HORIZONTAL = 0,
    MIKAN_INPUT_WHEEL_VERTICAL = 1
};

/* One canonical device event: plain data, read and written by value.
 *
 *   KEY_* / MOUSE_*  code is the key or button code; x and y are unused.
 *   MOUSE_MOVE       x, y is the absolute cursor position in pixels.
 *   WHEEL            x, y is the scroll delta (already summed per batch).
 *   FOCUS_*          code, x and y are unused.
 *
 * A MOUSE_MOVE carries no delta: consumers derive deltas from successive
 * positions, and the platform layer keeps its own per-frame delta. */
typedef struct mikan_input_event {
    uint32_t type;
    uint32_t code;
    double x;
    double y;
} mikan_input_event;

/* ------------------------------------------------------------ statuses */

enum {
    MIKAN_INPUT_OK = 0,
    MIKAN_INPUT_BAD_ARGUMENT = 1,
    MIKAN_INPUT_NOT_FOUND = 2,
    MIKAN_INPUT_CONFLICT = 3,
    MIKAN_INPUT_OVERFLOW = 4,
    MIKAN_INPUT_UNKNOWN_CONTEXT = 5
};

/* ------------------------------------------------- C: the device backend */

typedef struct mikan_input_backend mikan_input_backend;

/* Creates a backend with an event buffer of the given capacity; capacity
 * 0 selects the default (4096). Returns NULL on allocation failure. */
mikan_input_backend *mikan_input_backend_create(size_t capacity);

/* Destroys a backend; NULL is a no-op. */
void mikan_input_backend_destroy(mikan_input_backend *backend);

/* Feeds one key transition. `repeat` marks an operating-system key
 * repeat. Normalization: a press while the key is already down, a
 * release or repeat while it is not, are counted and dropped; a repeat
 * while the key is down becomes MIKAN_INPUT_EVENT_KEY_REPEAT.
 * Returns MIKAN_INPUT_OVERFLOW if the event was produced but the buffer
 * is full (the event is dropped and counted). */
int mikan_input_backend_push_key(mikan_input_backend *backend, uint32_t code,
                                 int pressed, int repeat);

/* Feeds one mouse-button transition; normalization as for keys. */
int mikan_input_backend_push_mouse_button(mikan_input_backend *backend,
                                          uint32_t code, int pressed);

/* Feeds one absolute cursor move. Non-finite coordinates are rejected
 * with MIKAN_INPUT_BAD_ARGUMENT. Consecutive moves are coalesced: only
 * the latest position is buffered. */
int mikan_input_backend_push_mouse_move(mikan_input_backend *backend,
                                        double x, double y);

/* Feeds one wheel scroll. Non-finite deltas are rejected with
 * MIKAN_INPUT_BAD_ARGUMENT. Consecutive scrolls are coalesced by
 * summing their deltas. */
int mikan_input_backend_push_wheel(mikan_input_backend *backend, double dx,
                                   double dy);

/* Feeds one focus change. Gaining focus emits FOCUS_GAINED. Losing
 * focus emits one release per held key and button (in device order),
 * then FOCUS_LOST, and clears the raw device state, so nothing can stay
 * stuck down while the window is in the background. Repeating the
 * current focus state is a no-op. */
int mikan_input_backend_push_focus(mikan_input_backend *backend, int focused);

/* 1 if the key is currently down in the raw device state. */
int mikan_input_backend_is_key_down(const mikan_input_backend *backend,
                                    uint32_t code);

/* 1 if the mouse button is currently down in the raw device state. */
int mikan_input_backend_is_mouse_button_down(
    const mikan_input_backend *backend, uint32_t code);

/* Number of buffered events; 0 for a NULL backend. */
size_t mikan_input_backend_len(const mikan_input_backend *backend);

/* How many events were dropped because the buffer was full, over the
 * backend's whole lifetime. */
size_t mikan_input_backend_dropped(const mikan_input_backend *backend);

/* Drains up to `capacity` buffered events into `out_events` in stream
 * order and reports how many were copied through `out_count`. Drained
 * events are consumed; events that do not fit stay buffered for the
 * next call. */
int mikan_input_backend_read(mikan_input_backend *backend,
                             mikan_input_event *out_events, size_t capacity,
                             size_t *out_count);

/* Drops every buffered event. Device state is untouched. */
int mikan_input_backend_clear(mikan_input_backend *backend);

/* ------------------------------------------- C++: the action runtime */

typedef struct mikan_input_runtime mikan_input_runtime;

/* One action's state after the events processed so far in this step.
 *
 *   held          at least one bound input is currently down.
 *   just_pressed  the action went from released to held in this step.
 *   just_released it went from held to released in this step.
 *   repeating     a bound key repeated while held in this step.
 *   value         1.0 while held plus the wheel deltas bound to it this
 *                 step; 0.0 when idle. Cleared by end_step.
 */
typedef struct mikan_input_action_state {
    int held;
    int just_pressed;
    int just_released;
    int repeating;
    float value;
} mikan_input_action_state;

/* Creates a runtime; returns NULL on allocation failure. */
mikan_input_runtime *mikan_input_runtime_create(void);

/* Destroys a runtime; NULL is a no-op. */
void mikan_input_runtime_destroy(mikan_input_runtime *runtime);

/* Adds a context and returns its id through `out_context`. Context ids
 * are assigned in creation order starting at 1. `priority` breaks
 * conflicts between active contexts: the higher priority wins; ties
 * prefer the context created later. */
int mikan_input_runtime_add_context(mikan_input_runtime *runtime,
                                    uint32_t priority, uint32_t *out_context);

/* Activates or deactivates a context. Deactivating releases everything
 * the context held down (with the usual just-released edges); nothing
 * auto-presses on activation. */
int mikan_input_runtime_set_context_active(mikan_input_runtime *runtime,
                                           uint32_t context, int active);

/* Removes a context. It must be inactive. Its bindings go with it. */
int mikan_input_runtime_remove_context(mikan_input_runtime *runtime,
                                       uint32_t context);

/* Binds `action` to one device input inside `context`. `device` is a
 * MIKAN_INPUT_EVENT_* constant of the matching kind (KEY_PRESS for a
 * key, MOUSE_PRESS for a button, WHEEL for a wheel axis); `code` is the
 * key, button, or wheel-axis code (wheel axes are 0 and 1 and are the
 * one device kind where code 0 is meaningful). Binding the same input
 * twice in one context is MIKAN_INPUT_CONFLICT; key or button code 0
 * and unknown device kinds are MIKAN_INPUT_BAD_ARGUMENT. */
int mikan_input_runtime_bind(mikan_input_runtime *runtime, uint32_t context,
                             uint32_t device, uint32_t code, uint32_t action);

/* Removes the binding of one device input from a context. */
int mikan_input_runtime_unbind(mikan_input_runtime *runtime, uint32_t context,
                               uint32_t device, uint32_t code);

/* Number of bindings in a context. */
int mikan_input_runtime_binding_count(const mikan_input_runtime *runtime,
                                      uint32_t context, size_t *out_count);

/* Processes one batch of normalized events, in stream order. Events
 * before the first input are applied first; an input is consumed by the
 * highest-priority active context that binds it. A wheel event carries
 * both axis deltas, and each axis is matched independently (an axis
 * binding consumes its own delta). */
int mikan_input_runtime_process(mikan_input_runtime *runtime,
                                const mikan_input_event *events,
                                size_t count);

/* Ends the current step: clears the just-pressed, just-released, and
 * repeating flags and the wheel contribution to `value`. Held state
 * persists until the matching release. */
int mikan_input_runtime_end_step(mikan_input_runtime *runtime);

/* Reads one action's state. An action with no bindings reads as idle
 * (MIKAN_INPUT_OK, all zeros); a null `out_state` is BAD_ARGUMENT. */
int mikan_input_runtime_action_state(const mikan_input_runtime *runtime,
                                     uint32_t action,
                                     mikan_input_action_state *out_state);

#ifdef __cplusplus
}
#endif

#endif /* MIKAN_INPUT_H */
