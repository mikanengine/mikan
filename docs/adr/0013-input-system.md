# 0013. Input system: a C device backend and a hand-written C++ action runtime

- Status: Accepted
- Date: 2026-10-17

## Context

The milestone after scene and entity is the **input system**: the layer that
turns window events into answers gameplay can read in a fixed update. Input
has one foot in every layer — the window produces raw events, the frame loop
consumes them once per step, and gameplay wants names for outputs ("quit",
"move left") — so this was always going to be a cross-cutting design, not a
drop-in crate.

The project-owner directive from Phase 5 stands: Mikan's stated architecture —
Rust + C++ forming the engine core, C as the low-level foundation — is a
requirement, not a long-term intention. Collision, Luau, and the scene graph
each took a bite of that (ADR 0010, 0011, 0012); input is the first milestone
designed *from the start* as a native two-layer split, and the first place
where the question "C or C++?" had a genuine third answer (the semantics could
have lived entirely in Rust).

Open questions the milestone had to answer:

1. **Where does the Rust/C/C++ line sit?** The honest options: all-Rust (the
   entire system is a `HashSet` and a `HashMap`; fast, but leaves the native
   claim resting only on vendored Luau and the scene graph), split (device
   handling in one language, actions in another), or all-native. The split
   chosen follows the data: **C owns the device backend** (the canonical event
   model, raw device state, and event normalization — state-machine work over
   plain structs, exactly C's confirmed home), **C++ owns the action
   runtime** (bindings grouped into prioritized contexts and the per-action
   state machine with arbitration — the kind of rule table that benefited
   from map/set abstractions in the scene graph), and **Rust owns
   everything else** — the platform event log, translation from winit onto
   the native codes, safe handles, frame integration, and the error types.
2. **What shape is the boundary?** Same discipline as the scene module: two
   opaque handles (`mikan_input_backend`, `mikan_input_runtime`), every entry
   point `extern "C"` with plain data in and a status code out, and — the key
   move — the *two native layers never call each other*. Rust translates
   winit events into normalized pushes, drains the backend buffer, and feeds
   the runtime in **one batch per fixed step**, so the boundary crosses only
   plain data, no C++ types or exceptions are ever visible, and allocation
   failure is fatal in a module built with `-fno-exceptions -fno-rtti`.
3. **What exactly is normalized?** Duplicate presses while held and
   releases/repeats of keys that are not down are counted and dropped;
   operating-system repeats become `KEY_REPEAT` only while the key is down;
   consecutive cursor moves are coalesced (latest wins); consecutive wheel
   deltas are **summed**; losing focus synthesizes one release per held device
   and a `FOCUS_LOST`, so nothing sticks while the window is in the
   background. A bounded buffer (1024 events/frame at the platform layer,
   4096 in the native backend) drops the newest and counts, instead of
   blocking or failing silently.
4. **How is arbitration defined exactly?** Per event, not per context: the
   highest-priority *active* context that binds an input consumes it; ties
   prefer the context created later. One input inside one context is a
   conflict; destruction requires inactivity; deactivation releases what the
   context held on; activation presses nothing on its own. Actions: `held`,
   `just_pressed`, `just_released`, `repeating`, and `value` (`held ? 1.0 :
   0.0` plus this step's bound wheel deltas); wheel produces **no press
   edges**, and `end_step` recomputes `value` exactly while held state
   persists until its release. That removes the two classic action-mapping
   bugs: stuck keys after focus loss, and wheel values that leak across
   steps.
5. **What stays out?** Gamepads (a later device kind), IME/text input (raw
   lanes stay physical), key-label/layout mapping, hot-reload of bindings
   (create fresh contexts instead), and any second thread. A documentation
   gate, not a stub: none of those have compatible half-interfaces today.

The measurement question, resolved the way ADR 0012 resolved it:

- The driver is **the architecture directive, not a profiled bottleneck**. An
  action lookup over thirty keys is not a bottleneck; the honest expectation
  is parity or worse and the measurement below reports what it is.
- ADR 0004 remains the policy for modules proposed *on performance grounds*.
  For this module the driver is architectural; the need, the boundary design,
  the measurement plan, and the verification plan are stated as `docs/ffi.md`
  requires, and the result is reported as measured.

## Decision

1. **Two new crates, following the required structure in `docs/ffi.md`:**
   - `crates/input-sys` — the native module: `src/input_backend.c` (C, the
     device backend), `src/input_runtime.cpp` (C++, the action runtime), two
     headers (`mikan_input.h` is the contract; two more expose driver
     internals to tests), a `build.rs` that compiles each with
     warnings-as-errors (C on the C driver, C++ on the C++ driver with
     `-fno-exceptions -fno-rtti`), the event/status mirrors, RAII handles
     with correct `Drop`, and `reference` — an independent Rust `Normalizer`
     that mirrors the backend's normalization in pure Rust.
   - `crates/input` — the safe wrapper and everything else: `InputSystem`
     (owns backend + runtime, translates platform events, maps statuses to
     `InputError`s), `ActionId`/`ContextId`/`Binding`/`WheelAxis`, and the
     `map` module (winit `PhysicalKey`/`MouseButton` → HID codes). It
     contains no `unsafe`.
2. **The surfaces are one opaque type each.** The backend's API is push
   (one normalized event per transition, coalesced where stated), `read`
   (bounded drain), and `clear`; the runtime's API is contexts, bindings,
   `process(events, count)`, `end_step`, and `action_state(action)`. No
   shared mutable state between them; Rust is the only bridge.
3. **The safe layer adds the engine semantics.** `InputSystem` maps status
   codes onto `InputError` (with the platform-flow distinction: `UnknownKey`
   on an unbindable code, `DuplicateBinding` on conflict, `BindingNotFound`,
   `UnknownContext`, `ContextActive`, `Overflow{dropped}`, `Native` for
   runtime-internal failures) and takes `&[DeviceEvent]` — the platform's
   own event log type — so no winit dependency leaks past `crates/input`.
4. **The platform is the front door, unchanged by the native work.** The
   per-frame `DeviceEvent` log (already there for Phase 3's raw lane) gains
   only a drain and a wheel-tick normalization; `InputState` stays the raw
   lane, and `AppContext` now lends `&mut InputState` so a step's fixed
   update can both read held state and drain the same events into the action
   lane. The facade and `Game` trait do not bend.
5. **A Rust reference implementation is kept — deliberately.** The Rust
   `Normalizer` mirrors the C backend's normalization so the two are
   cross-checked **bit for bit** on randomized streams (the same oracle pattern
   as collision and scene). Because the runtime has no meaningful Rust twin to
   test against, its tests instead drive it directly and assert the C++
   semantics (edges, priorities, wheel values, focus cleanup) one behavior per
   test.
6. **The native module is "done" only per `docs/ffi.md`'s checklist**: tests
   through the wrapper exercise both native layers including all the error
   paths; the sources build warnings-as-errors; doc and integration tests
   exist at every level; `docs/ARCHITECTURE.md` places the module in the
   dependency graph; a Valgrind run of the benchmark binary is recorded in
   `docs/STATUS.md`; and the CI smoke job now ends with the action-mapping
   verify script.
7. **The measurement is reported as measured.** The recorded run shows the
   engine step at ≈ 36 ns/event on a 9-event batch, the C backend at ≈ 16
   ns/event, and the Rust oracle at ≈ 11 ns/event — **the native kernel is
   slower than the reference here**, and that is the deliberate honest result
   (Criterion 3 of ADR 0004 is *not* claimed). The runtime and translation add
   ~0.2 µs/batch on top of the backend, and the cross-check makes correctness,
   not speed, the module's contract with itself.
8. **The demo demonstrates the whole path.** `demo2d --actions` drives the
   simulation from action state files (global context: Escape → quit;
   gameplay: WASD/arrows, `F`, wheel-y) entirely through the platform event
   log, while the raw lane keeps running so the two can be compared in one
   capture. The classic scene's controls are unaffected unless `--actions`.

## Consequences

Positive:

- Gameplay code can now ask **what the player meant** (actions, contexts,
  analog-ish values) instead of what a key did, and two lanes can coexist in
  one executable — demo2d proves it.
- The device model is a small, documented C surface over plain data; the
  action model is a C++ state table behind the same discipline; the two never
  reach into each other. A future gamepad is a new `push_*` pair and a new
  device kind, not a redesign.
- Every normalization rule is enforced exactly once and cross-checked against
  an independent Rust implementation; the recorded Valgrind run is clean.
- The platform's event log gained a drop-counting bounded mode and wheel
  normalization; the raw lane did not change behavior.

Negative:

- **The C backend measures slower than the Rust oracle** (≈16 ns/event vs
  ≈11 ns/event in the recorded run; the full engine step stays under
  ≈40 ns/event). As with the scene graph, this is the honest measured result,
  not a hidden one: the driver was architectural, and the module's value is
  the ABI and the two-language split the architecture claims.
- Two new sources of FFI bugs exist now; they are fenced by the same recipe as
  the first three native modules (status codes in, `Result` out, RAII, no
  exceptions), plus a new one: **the two native layers never call each
  other**, so neither can import the other's bugs, and one translation point
  (Rust) owns all cross-cutting correctness.
- `unsafe` now exists in three crates (`mikan-collision-sys`,
  `mikan-scene-sys`, `mikan-input-sys`); the workspace lint stays `forbid`
  everywhere else.
- Wheel values and press edges keep their strict one-step windows; code that
  wants "scroll accumulated this session" must keep its own sum, and text
  input is still future work.
