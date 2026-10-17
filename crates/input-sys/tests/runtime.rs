//! Tests for the C++ action runtime, exercised through the safe RAII
//! wrapper.
//!
//! Covered: press/release/held/repeat transitions, multiple bindings per
//! action, binding conflicts and validation, context activation with
//! precedence, focus-loss sweeps, event ordering and duplicate prevention,
//! wheel (analog) values, and the failure paths (unknown contexts, unbind of
//! a missing binding, removal of an active context).

use mikan_input_sys::{ActionState, InputEvent, Runtime, RuntimeError, keys};

/// The runtime equivalent of a fixed step: process, query, end.
fn step(runtime: &mut Runtime, events: &[InputEvent]) {
    runtime.process(events).unwrap();
    runtime.end_step().unwrap();
}

/// The idle state, with the explicit value 0.0.
fn idle() -> ActionState {
    ActionState::default()
}

/// Held and just-pressed this step.
fn pressed() -> ActionState {
    ActionState {
        held: true,
        just_pressed: true,
        value: 1.0,
        ..idle()
    }
}

/// Held without an edge (the common state between press and release).
fn held() -> ActionState {
    ActionState {
        held: true,
        value: 1.0,
        ..idle()
    }
}

/// Just released this step.
fn released() -> ActionState {
    ActionState {
        just_released: true,
        ..idle()
    }
}

#[test]
fn press_release_and_held_transitions() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();

    runtime
        .process(&[InputEvent::key(keys::KEY_A, true)])
        .unwrap();
    assert_eq!(runtime.action_state(1).unwrap(), pressed());

    step(&mut runtime, &[]);
    assert_eq!(
        runtime.action_state(1).unwrap(),
        held(),
        "held state persists; the edge is per step"
    );

    runtime
        .process(&[InputEvent::key(keys::KEY_A, false)])
        .unwrap();
    assert_eq!(runtime.action_state(1).unwrap(), released());

    step(&mut runtime, &[]);
    assert_eq!(runtime.action_state(1).unwrap(), idle(), "idle again");
}

#[test]
fn repeats_mark_repeating_but_never_new_press_edges() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_D, 1).unwrap();

    runtime
        .process(&[
            InputEvent::key(keys::KEY_D, true),
            InputEvent::key_repeat(keys::KEY_D),
            InputEvent::key_repeat(keys::KEY_D),
        ])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(state.held && state.just_pressed && state.repeating);
    assert_eq!(state.value, 1.0);

    // end_step clears the edges; held state persists.
    runtime.end_step().unwrap();
    assert_eq!(runtime.action_state(1).unwrap(), held());

    // A repeat in the next step, with no new press: repeating again.
    runtime
        .process(&[InputEvent::key_repeat(keys::KEY_D)])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        state.held && !state.just_pressed && state.repeating,
        "repeat without a press"
    );

    // end_step clears the flag; held state persists.
    runtime.end_step().unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        state.held && !state.repeating,
        "repeating is cleared by end_step"
    );
    assert_eq!(state.value, 1.0);
}

#[test]
fn multiple_bindings_or_into_one_action() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();
    runtime.bind_key(context, keys::KEY_ARROWLEFT, 1).unwrap();
    runtime
        .bind_mouse_button(context, mikan_input_sys::MOUSE_LEFT, 1)
        .unwrap();
    assert_eq!(runtime.binding_count(context).unwrap(), 3);

    runtime
        .process(&[InputEvent::key(keys::KEY_A, true)])
        .unwrap();
    assert!(runtime.is_action_held(1).unwrap());

    // A second binding going down does not re-fire the edge.
    step(&mut runtime, &[InputEvent::key(keys::KEY_ARROWLEFT, true)]);
    let state = runtime.action_state(1).unwrap();
    assert!(
        state.held && !state.just_pressed,
        "the action was already held"
    );

    // One of two held bindings releasing keeps the action held.
    runtime
        .process(&[InputEvent::key(keys::KEY_A, false)])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(state.held && !state.just_released);

    runtime
        .process(&[InputEvent::key(keys::KEY_ARROWLEFT, false)])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        !state.held && state.just_released,
        "the last holder releases the action"
    );

    // The mouse binding was never touched.
    step(&mut runtime, &[]);
    assert_eq!(runtime.action_state(1).unwrap(), idle());
}

#[test]
fn binding_conflicts_and_invalid_configurations_are_rejected() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();

    runtime.bind_key(context, keys::KEY_A, 1).unwrap();
    assert_eq!(
        runtime.bind_key(context, keys::KEY_A, 2),
        Err(RuntimeError::Conflict),
        "the same input cannot drive two actions in one context"
    );
    assert_eq!(
        runtime.bind_key(context, keys::KEY_A, 1),
        Err(RuntimeError::Conflict),
        "not even to the same action twice"
    );
    assert_eq!(
        runtime.bind_key(context, keys::KEY_D, 2),
        Ok(()),
        "other inputs bind fine"
    );

    assert_eq!(
        runtime.bind_key(context, keys::KEY_UNKNOWN, 3),
        Err(RuntimeError::BadArgument)
    );
    assert_eq!(
        runtime.bind_mouse_button(context, mikan_input_sys::MOUSE_NONE, 3),
        Err(RuntimeError::BadArgument)
    );
    assert_eq!(
        runtime.bind_wheel(context, 7, 3),
        Err(RuntimeError::BadArgument),
        "wheel axes are 0 and 1 only"
    );
    assert_eq!(
        runtime.bind(context, 42, keys::KEY_A, 3),
        Err(RuntimeError::BadArgument),
        "unknown device kinds are rejected"
    );
    assert_eq!(
        runtime.add_context(0).unwrap(),
        2,
        "context ids count from 1"
    );
    assert_eq!(
        runtime.bind_key(99, keys::KEY_A, 1),
        Err(RuntimeError::UnknownContext)
    );

    // The conflict left nothing bound twice: one binding for KEY_A.
    assert_eq!(runtime.binding_count(context).unwrap(), 2);
}

#[test]
fn unbind_releases_a_held_binding_with_its_edge() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_A, true)])
        .unwrap();
    assert!(runtime.is_action_held(1).unwrap());

    runtime
        .unbind(context, mikan_input_sys::EVENT_KEY_PRESS, keys::KEY_A)
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        !state.held && state.just_released,
        "unbinding a held input releases it"
    );

    assert_eq!(
        runtime.unbind(context, mikan_input_sys::EVENT_KEY_PRESS, keys::KEY_A),
        Err(RuntimeError::NotFound)
    );
    step(&mut runtime, &[]);
    assert_eq!(runtime.action_state(1).unwrap(), ActionState::default());
}

#[test]
fn contexts_have_explicit_priority_precedence() {
    let mut runtime = Runtime::new();
    let gameplay = runtime.add_context(0).unwrap();
    let ui = runtime.add_context(100).unwrap();
    runtime.bind_key(gameplay, keys::KEY_ESCAPE, 1).unwrap();
    runtime.bind_key(ui, keys::KEY_ESCAPE, 2).unwrap();

    // Both active: the higher priority consumes the input.
    runtime
        .process(&[InputEvent::key(keys::KEY_ESCAPE, true)])
        .unwrap();
    assert!(
        runtime.is_action_just_pressed(2).unwrap(),
        "the UI context wins"
    );
    assert!(
        !runtime.is_action_just_pressed(1).unwrap(),
        "gameplay never saw it"
    );

    runtime
        .process(&[InputEvent::key(keys::KEY_ESCAPE, false)])
        .unwrap();
    step(&mut runtime, &[]);

    // UI deactivated: the same input now reaches gameplay.
    runtime.set_context_active(ui, false).unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_ESCAPE, true)])
        .unwrap();
    assert!(runtime.is_action_just_pressed(1).unwrap());
    assert!(
        !runtime.is_action_just_pressed(2).unwrap(),
        "deactivation is not auto-press"
    );

    // A tie prefers the context created later.
    let later = runtime.add_context(100).unwrap();
    runtime.bind_key(later, keys::KEY_ESCAPE, 3).unwrap();
    runtime.set_context_active(ui, true).unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_ESCAPE, false)])
        .unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_ESCAPE, true)])
        .unwrap();
    assert!(
        runtime.is_action_just_pressed(3).unwrap(),
        "the later context wins the tie"
    );
    assert!(!runtime.is_action_just_pressed(2).unwrap());
}

#[test]
fn release_reaches_the_context_that_consumed_the_press() {
    let mut runtime = Runtime::new();
    let gameplay = runtime.add_context(0).unwrap();
    let ui = runtime.add_context(100).unwrap();
    runtime.bind_key(gameplay, keys::KEY_A, 1).unwrap();
    runtime.bind_key(ui, keys::KEY_A, 2).unwrap();
    runtime.set_context_active(ui, false).unwrap();

    runtime
        .process(&[InputEvent::key(keys::KEY_A, true)])
        .unwrap();
    assert!(runtime.is_action_held(1).unwrap());
    runtime.end_step().unwrap();

    runtime.set_context_active(ui, true).unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_A, false)])
        .unwrap();

    let gameplay_state = runtime.action_state(1).unwrap();
    assert!(!gameplay_state.held);
    assert!(gameplay_state.just_released);
    assert_eq!(
        runtime.action_state(2).unwrap(),
        ActionState::default(),
        "the newly active context never received the press"
    );
}

#[test]
fn deactivating_a_context_releases_what_it_held() {
    let mut runtime = Runtime::new();
    let gameplay = runtime.add_context(0).unwrap();
    let debug = runtime.add_context(1).unwrap();
    runtime.bind_key(gameplay, keys::KEY_W, 1).unwrap();
    runtime.bind_key(debug, keys::KEY_F3, 2).unwrap();

    runtime
        .process(&[InputEvent::key(keys::KEY_W, true)])
        .unwrap();
    runtime
        .process(&[InputEvent::key(keys::KEY_F3, true)])
        .unwrap();
    assert!(runtime.is_action_held(1).unwrap() && runtime.is_action_held(2).unwrap());

    // Deactivating gameplay releases only its own held action.
    runtime.set_context_active(gameplay, false).unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        !state.held && state.just_released,
        "the held action was released"
    );
    assert!(
        runtime.is_action_held(2).unwrap(),
        "other contexts are untouched"
    );

    // Releasing the still-held input of the inactive context is a no-op:
    // the inactive context no longer consumes it.
    runtime
        .process(&[InputEvent::key(keys::KEY_F3, false)])
        .unwrap();
    step(&mut runtime, &[]);
    assert!(
        !runtime.action_state(2).unwrap().held,
        "the debug action must not stay held forever"
    );
}

#[test]
fn context_removal_and_unknown_contexts() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();
    runtime.set_context_active(context, false).unwrap();
    runtime.remove_context(context).unwrap();
    assert_eq!(
        runtime.remove_context(context),
        Err(RuntimeError::UnknownContext)
    );

    let active = runtime.add_context(0).unwrap();
    assert_eq!(
        runtime.remove_context(active),
        Err(RuntimeError::BadArgument)
    );
    assert_eq!(
        runtime.set_context_active(active + 1000, true),
        Err(RuntimeError::UnknownContext)
    );
    assert_eq!(
        runtime.binding_count(active + 1000),
        Err(RuntimeError::UnknownContext)
    );
}

#[test]
fn focus_loss_sweeps_every_held_action() {
    let mut runtime = Runtime::new();
    let gameplay = runtime.add_context(0).unwrap();
    runtime.bind_key(gameplay, keys::KEY_W, 1).unwrap();
    runtime
        .bind_mouse_button(gameplay, mikan_input_sys::MOUSE_LEFT, 2)
        .unwrap();

    // The backend synthesizes releases on focus loss; feed the same shape
    // the real pipeline produces.
    runtime
        .process(&[
            InputEvent::key(keys::KEY_W, true),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_LEFT, true),
            InputEvent::key(keys::KEY_W, false),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_LEFT, false),
            InputEvent::focus(false),
        ])
        .unwrap();
    assert!(!runtime.is_action_held(1).unwrap());
    assert!(!runtime.is_action_held(2).unwrap());
    assert!(runtime.is_action_just_released(1).unwrap());

    // Defense in depth: FOCUS_LOST alone also clears anything left.
    runtime
        .process(&[InputEvent::key(keys::KEY_W, true)])
        .unwrap();
    assert!(runtime.is_action_held(1).unwrap());
    runtime.process(&[InputEvent::focus(false)]).unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        !state.held && state.just_released,
        "the sweep releases what is left"
    );

    // Unknown events are ignored, not fatal.
    runtime
        .process(&[InputEvent {
            kind: 9999,
            code: 0,
            x: 0.0,
            y: 0.0,
        }])
        .unwrap();
}

#[test]
fn wheel_bindings_carry_an_analog_value() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime
        .bind_wheel(context, mikan_input_sys::WHEEL_VERTICAL, 1)
        .unwrap();
    runtime.bind_key(context, keys::KEY_SPACE, 2).unwrap();

    runtime.process(&[InputEvent::wheel(0.0, 1.0)]).unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(!state.held && !state.just_pressed, "wheel never presses");
    assert_eq!(state.value, 1.0, "the step's wheel sum is the analog value");

    runtime.process(&[InputEvent::wheel(0.0, -3.0)]).unwrap();
    assert_eq!(runtime.action_state(1).unwrap().value, -2.0, "deltas sum");

    step(&mut runtime, &[]);
    assert_eq!(
        runtime.action_state(1).unwrap().value,
        0.0,
        "the wheel value is per step"
    );

    // A held digital action reads 1.0 even while unbound wheel actions read 0.
    runtime
        .process(&[InputEvent::key(keys::KEY_SPACE, true)])
        .unwrap();
    assert_eq!(runtime.action_state(2).unwrap().value, 1.0);

    // An unconsumed wheel event (no binding for the horizontal axis) changes
    // nothing: the vertical axis stays at its idle value.
    runtime.process(&[InputEvent::wheel(5.0, 0.0)]).unwrap();
    assert_eq!(runtime.action_state(1).unwrap().value, 0.0);
    assert_eq!(runtime.action_state(2).unwrap().value, 1.0);
}

#[test]
fn event_order_is_respected_within_a_batch() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();

    // Press and release in one batch: both edges fire, held ends false.
    runtime
        .process(&[
            InputEvent::key(keys::KEY_A, true),
            InputEvent::key(keys::KEY_A, false),
        ])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(state.just_pressed && state.just_released && !state.held);

    // The reverse order leaves no stale held state: the ignored release
    // changes nothing, and the later press still creates its edge.
    step(&mut runtime, &[]);
    runtime
        .process(&[
            InputEvent::key(keys::KEY_A, false),
            InputEvent::key(keys::KEY_A, true),
        ])
        .unwrap();
    let state = runtime.action_state(1).unwrap();
    assert!(
        state.held && state.just_pressed,
        "the ignored release did not block the press"
    );
}

#[test]
fn unknown_and_unbound_actions_read_as_idle() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.action_state(12345).unwrap(), ActionState::default());
    step(&mut runtime, &[]);
    assert_eq!(runtime.action_state(0).unwrap(), ActionState::default());

    // A bound action that was never pressed also reads idle.
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 7).unwrap();
    assert_eq!(runtime.action_state(7).unwrap(), ActionState::default());
}

#[test]
fn mouse_moves_and_unknown_codes_change_nothing() {
    let mut runtime = Runtime::new();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();

    runtime
        .process(&[InputEvent::mouse_move(100.0, 200.0)])
        .unwrap();
    assert_eq!(runtime.action_state(1).unwrap(), ActionState::default());
    assert_eq!(
        runtime.action_state(2).unwrap(),
        ActionState::default(),
        "no action exists"
    );
}

#[test]
fn empty_batches_and_repeat_edge_cases_are_noops() {
    let mut runtime = Runtime::new();
    runtime.process(&[]).unwrap();
    let context = runtime.add_context(0).unwrap();
    runtime.bind_key(context, keys::KEY_A, 1).unwrap();
    // A repeat of an unheld key is dropped (the backend normalizes this;
    // the runtime defends against it too).
    runtime
        .process(&[InputEvent::key_repeat(keys::KEY_A)])
        .unwrap();
    assert_eq!(runtime.action_state(1).unwrap(), ActionState::default());
}
