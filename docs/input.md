# Input: from window events to actions

Mikan's input system turns raw operating-system events into gameplay questions.
It answers five: what happened (the canonical event model), what is true about
the devices right now (`InputState`, the raw lane), which inputs map to which
actions (bindings), which bindings outrank each other (contexts and
priorities), and what state an action carried this step (the action state
machine). Device handling is a C backend and action handling is a C++ runtime;
Rust owns configuration, safe handles, and frame integration. The decision
record — including why the native layer exists and its honest measurement — is
[ADR 0013](adr/0013-input-system.md).

## Crates and layers

| Crate | Role |
| --- | --- |
| `crates/input-sys` | The native module: `src/input_backend.c` (C, the device backend: canonical events, raw device state, normalization), `src/input_runtime.cpp` (C++, the action runtime: bindings, contexts, priorities, action states), `src/mikan_input.h` (the contract), a `build.rs` that compiles each driver with warnings-as-errors (`-fno-exceptions -fno-rtti` for C++), the event/status mirrors, RAII handles, and `reference` — the independent Rust normalizer used as the cross-check oracle and benchmark baseline. |
| `crates/input` | The safe face: `InputSystem`, `ActionId`, `ContextId`, `Binding`, `WheelAxis`, and the `map` module that translates winit keys and buttons onto the native codes. Contains no `unsafe`. |
| `crates/platform` | The window-event source: `InputState` records a per-frame `DeviceEvent` log that the input layer drains. Never touches the native module. |

Three layers, in call order per fixed step:

1. **Device backend (C).** `mikan_input_backend` owns the canonical event
   model, a bounded event buffer, and the raw device state. It normalizes the
   raw stream: duplicate presses and releases of keys that are not down are
   suppressed, operating-system repeats become `EVENT_KEY_REPEAT` only while
   the key is down, consecutive cursor moves are coalesced, consecutive wheel
   deltas are coalesced by summation, losing focus synthesizes one release per
   held device and a `FOCUS_LOST` event.
2. **Action runtime (C++).** `mikan_input_runtime` consumes the normalized
   batch and maintains the action state machines. Physical bindings map to
   actions; actions live in contexts with priorities; each event is consumed
   once.
3. **Engine integration (Rust).** `InputSystem` owns one backend and one
   runtime, translates `DeviceEvent`s through `map`, renders status codes as
   `InputError`s, and exposes the queries a fixed update reads. Callers never
   see events, statuses, or pointers.

The two native layers never call each other: Rust drains the backend's buffer
and feeds the runtime in one batch per step, so the boundary crosses only plain
data.

## The canonical event model

An event is a plain four-field struct: `type` (key press/release/repeat, mouse
press/release/move, wheel, focus gained/lost), `code`, and two `f64` values.
Key codes are USB HID keyboard usage IDs (page 1), so `KEY_A` is `4`,
`KEY_ESCAPE` is `41`, `KEY_METARIGHT` is `231`: a stable, documented numbering
that maps one-to-one onto engine key names. Mouse buttons are `1` (left), `2`
(right), `3` (middle), and `256 + n` for numbered extra buttons; without a
documented HID numbering, back/forward count as other buttons. Wheel axes are
codes `0` (horizontal) and `1` (vertical) — the one device kind where code `0`
is meaningful, and `KEY_UNKNOWN` (`0`) is reserved as unbindable for keys.

## The platform event log (what feeds it)

`mikan-platform` records every window event into a per-frame `DeviceEvent` log
(keys, mouse buttons, cursor moves, wheel ticks, focus changes), in stream
order. `AppContext::input()` yields the state for the raw lane (held/edges,
cursor delta) and `input_mut().take_events()` **drains** the log for the
action lane. The log is bounded at 1024 events per frame; once full, the
newest events are dropped and counted (`dropped_events()`), instead of
stalling the window event loop. `LineDelta` wheel ticks are normalized to
pixels on the way in (one tick = 24 logical points), and `end_frame` clears
the log, wheel residual, and edges, so zero-step frames keep events but fixed
steps always start from whatever the previous drain left.

## Normalization: what the backend does

- A key press while the key is already down, and a release or repeat while it
  is not, are counted and dropped. A repeat while the key is down becomes
  `EVENT_KEY_REPEAT`.
- Cursor positions are absolute; consecutive moves are coalesced (only the
  latest position stays buffered), so a fast mouse crossing the window costs
  one event per step.
- Wheel deltas of consecutive scrolls are coalesced by **summing**, which is
  why a burst of two notches up is one event with `dy = -2.0`-scaled pixels.
- Losing focus emits one release per held key and button (device order), then
  `FOCUS_LOST`, and clears the raw device state — nothing stays stuck while
  the window is in the background. Repeating the current focus state is a
  no-op.
- The buffer is bounded (default capacity 4096); when full, the event is
  dropped and counted rather than failing.

## Actions, contexts, and priorities

- **Bindings** map one device input (key, button, wheel axis) to one action id
  inside one context. Two bindings for the same input inside one context is a
  `CONFLICT` (surfaced as `InputError::DuplicateBinding`).
- **Contexts** group bindings with a priority. `ContextId`s start at 1 and
  increase with creation order; they are generational-pool-safe (never reused)
  and contexts can be activated/deactivated/removed (removal requires the
  context to be inactive). Deactivating a context releases everything it held
  down (with the usual just-released edges); activating one never presses
  anything on its own.
- **Arbitration is per event**: the highest-priority *active* context that
  binds an input consumes it; lower contexts never see that event. Ties
  prefer the context created later. One input may therefore drive different
  actions in different moments.

Actions are plain `u32` ids (`ActionId`); meaning lives in the game's own
constants (demo2d's actions module gives them names like `QUIT` and
`MOVE_LEFT`). Contexts carry names in the safe layer, used for `Debug` output
and error messages only.

## Action state semantics

After the events of the current step are processed, each action reports:

- `held` — at least one bound input is currently down;
- `just_pressed` — it went released → held during this step;
- `just_released` — it went held → released during this step;
- `repeating` — a bound key repeated while held during this step;
- `value` — `1.0` while held, plus the sum of wheel deltas bound to it during
  this step, `0.0` when idle.

Two deliberate properties:

- **Wheel contributes no press edges.** A wheel event only adds to `value`
  (each axis binding consumes its own delta component); `just_pressed` never
  fires from a scroll.
- **`end_step` recomputes `value` exactly.** The wheel contribution is
  cleared, and `value` becomes `held ? 1.0 : 0.0`; whether the value drifts
  between the events and the query is therefore a one-step, well-defined
  window, not a leftover. Just-pressed/released/repeating flags are cleared
  too; held state persists until the matching release.

## The safe layer

`InputSystem::new()` creates both native halves. `create_context(name,
priority)` returns a `ContextId` and registers the name for error messages;
`bind/_process/end_step` mirror the native calls with Rust error types:

| Native status | Rust error |
| --- | --- |
| `BAD_ARGUMENT` (key code `0` for a key binding, unknown device kind) | `InputError::UnknownKey` |
| `CONFLICT` | `InputError::DuplicateBinding` |
| `NOT_FOUND` | `InputError::BindingNotFound` |
| `UNKNOWN_CONTEXT` | `InputError::UnknownContext` |
| removing an active context | `InputError::ContextActive` |
| dropped events | `InputError::Overflow` (carries the count) |
| a native runtime failure | `InputError::Native` |

`process(&[DeviceEvent])` translates the platform events (winit `PhysicalKey`
and `MouseButton` to HID codes via `map`), pushes them into the backend, then
drains and feeds the runtime in one batch. Unknown keys are translated to
code `0` and tracked raw — they simply cannot be bound. Queries
(`held/just_pressed/just_released/repeating/value/action_state`) read the
native state, and `dropped` reports how many events never made it (see
`Overflow` above).

## Engine integration and the demo's two lanes

`AppContext` lends `&mut InputState`, so an application can both read the raw
lane (`input()`) and drain the log (`input_mut().take_events()`). demo2d shows
both, gated by `--actions`:

- the **raw lane** (default) reads `is_key_held/is_key_just_pressed` like
  every earlier milestone;
- the **actions lane** (`--actions`) drains the log once per update, feeds
  `ActionMapping` (a thin wrapper over `InputSystem` with a `global` context —
  Escape → quit at priority 10 — and a `gameplay` context — WASD/arrows →
  movement, `F` → despawn the moon, wheel-y → scroll at priority 0), reads the
  queries, and calls `end_step` before the simulation advances.

No winit events are synthesized; the wheel path exercises the backend's
per-frame coalescing, the runtime's per-axis matching, and the value report.

## The measurement

`scripts/bench_input.sh` runs the same deterministic batch stream (6 keys, 2
cursor moves, 1 wheel; 65 536 batches × 9 events, 40 runs after warm-up) through:

- the **engine step** — translation + backend + runtime + queries +
  `end_step`;
- the **kernel** — the C backend alone (the same pushes + drain);
- the **reference** — the Rust `Normalizer` oracle on the same stream.

The recorded numbers on this machine (see `docs/STATUS.md` for the run):
engine step ≈ 36 ns/event, native backend ≈ 16 ns/event, Rust reference ≈
11 ns/event — **the C backend measures slower than the Rust oracle**; the
runtime and translation add the remainder. No speedup is claimed: the module
exists because the low-level device foundation is C's assigned home and the
action state machine/context arbitration is C++'s (ADR 0013). A cross-check
test proves the backend and the reference agree bit for bit. A Valgrind run is
recorded in `docs/STATUS.md`.

## Limitations and extensions

- **No gamepad.** The device model covers keyboard, mouse, wheel, and focus.
  A pad is a later device kind (its analog sticks want a richer value model).
- **No text input.** Physical keys only; IME/text editing belongs to a future
  layer (the platform's `InputState` stays the raw-lane owner).
- **No key-label mapping.** The HID table maps physical codes; what the key
  prints is a layout question this system deliberately does not answer.
- **Single-threaded.** Each handle requires exclusive access, exactly as its
  Rust `&mut` wrapper models.
