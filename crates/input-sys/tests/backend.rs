//! Tests for the C device backend, exercised through the safe RAII wrapper
//! and, for the defensive paths, through the raw `extern "C"` declarations.
//!
//! Covered: the normalization rules (duplicates, spurious releases, repeats,
//! coalescing, focus-loss release synthesis), the raw device-state queries,
//! the bounded buffer (overflow drop counting, partial drains), lifecycle,
//! and the module's defensive error paths with null handles.

use mikan_input_sys::{
    BAD_ARGUMENT, Backend, BackendError, InputEvent, OK, keys, mikan_input_backend_clear,
    mikan_input_backend_destroy, mikan_input_backend_dropped, mikan_input_backend_is_key_down,
    mikan_input_backend_is_mouse_button_down, mikan_input_backend_len,
    mikan_input_backend_push_key, mikan_input_backend_read,
};

fn events(backend: &mut Backend) -> Vec<InputEvent> {
    let mut drained = Vec::new();
    backend.drain(&mut drained).unwrap();
    drained
}

#[test]
fn press_release_and_device_state_agree() {
    let mut backend = Backend::new(0);
    assert!(
        !backend.is_key_down(keys::KEY_A),
        "nothing is down at start"
    );

    backend.push_key(keys::KEY_A, true, false).unwrap();
    assert!(backend.is_key_down(keys::KEY_A));
    assert_eq!(
        events(&mut backend),
        vec![InputEvent::key(keys::KEY_A, true)]
    );

    backend.push_key(keys::KEY_A, false, false).unwrap();
    assert!(!backend.is_key_down(keys::KEY_A));
    assert_eq!(
        events(&mut backend),
        vec![InputEvent::key(keys::KEY_A, false)]
    );
}

#[test]
fn duplicate_presses_and_spurious_releases_are_normalized() {
    let mut backend = Backend::new(0);
    backend.push_key(keys::KEY_D, true, false).unwrap();
    backend.push_key(keys::KEY_D, true, false).unwrap();
    backend.push_key(keys::KEY_D, false, false).unwrap();
    backend.push_key(keys::KEY_D, false, false).unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_LEFT, true)
        .unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_LEFT, true)
        .unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_LEFT, false)
        .unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_LEFT, false)
        .unwrap();
    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::key(keys::KEY_D, true),
            InputEvent::key(keys::KEY_D, false),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_LEFT, true),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_LEFT, false),
        ]
    );
}

#[test]
fn repeats_fire_only_while_the_key_is_down() {
    let mut backend = Backend::new(0);
    backend.push_key(keys::KEY_D, true, false).unwrap();
    backend.push_key(keys::KEY_D, true, true).unwrap();
    backend.push_key(keys::KEY_D, false, false).unwrap();
    backend.push_key(keys::KEY_D, true, true).unwrap();
    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::key(keys::KEY_D, true),
            InputEvent::key_repeat(keys::KEY_D),
            InputEvent::key(keys::KEY_D, false),
        ]
    );
}

#[test]
fn mouse_moves_coalesce_to_the_latest_position() {
    let mut backend = Backend::new(0);
    backend.push_mouse_move(10.0, 20.0).unwrap();
    backend.push_mouse_move(30.0, 40.0).unwrap();
    backend.push_key(keys::KEY_W, true, false).unwrap();
    backend.push_mouse_move(50.0, 60.0).unwrap();
    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::mouse_move(30.0, 40.0),
            InputEvent::key(keys::KEY_W, true),
            InputEvent::mouse_move(50.0, 60.0),
        ]
    );
}

#[test]
fn wheel_scrolls_coalesce_by_summing() {
    let mut backend = Backend::new(0);
    backend.push_wheel(1.0, 0.0).unwrap();
    backend.push_wheel(0.0, -2.0).unwrap();
    backend.push_wheel(0.0, 0.0).unwrap();
    backend.push_key(keys::KEY_W, true, false).unwrap();
    backend.push_wheel(0.5, 0.25).unwrap();
    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::wheel(1.0, -2.0),
            InputEvent::key(keys::KEY_W, true),
            InputEvent::wheel(0.5, 0.25),
        ]
    );
}

#[test]
fn non_finite_positions_and_deltas_are_rejected() {
    let mut backend = Backend::new(0);
    assert_eq!(
        backend.push_mouse_move(f64::NAN, 0.0),
        Err(BackendError::BadArgument)
    );
    assert_eq!(
        backend.push_mouse_move(0.0, f64::INFINITY),
        Err(BackendError::BadArgument)
    );
    assert_eq!(
        backend.push_wheel(f64::NAN, 0.0),
        Err(BackendError::BadArgument)
    );
    assert_eq!(
        backend.push_wheel(0.0, f64::NEG_INFINITY),
        Err(BackendError::BadArgument)
    );
    assert!(backend.is_empty(), "rejected events left nothing buffered");
}

#[test]
fn focus_loss_releases_every_held_device_and_sweeps_state() {
    let mut backend = Backend::new(0);
    backend.push_focus(true).unwrap();
    backend.push_key(keys::KEY_W, true, false).unwrap();
    backend.push_key(keys::KEY_A, true, false).unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_RIGHT, true)
        .unwrap();
    backend.push_focus(false).unwrap();

    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::focus(true),
            InputEvent::key(keys::KEY_W, true),
            InputEvent::key(keys::KEY_A, true),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_RIGHT, true),
            InputEvent::key(keys::KEY_W, false),
            InputEvent::key(keys::KEY_A, false),
            InputEvent::mouse_button(mikan_input_sys::MOUSE_RIGHT, false),
            InputEvent::focus(false),
        ]
    );
    assert!(
        !backend.is_key_down(keys::KEY_W) && !backend.is_key_down(keys::KEY_A),
        "device state is cleared with the releases"
    );
    assert!(!backend.is_mouse_button_down(mikan_input_sys::MOUSE_RIGHT));

    // Repeating the current focus state is a no-op.
    backend.push_focus(false).unwrap();
    backend.push_focus(true).unwrap();
    backend.push_focus(true).unwrap();
    assert_eq!(
        events(&mut backend),
        vec![InputEvent::focus(true)],
        "gaining focus twice emits once"
    );
}

#[test]
fn overflow_drops_the_newest_event_and_counts_it() {
    let mut backend = Backend::new(2);
    backend.push_key(keys::KEY_A, true, false).unwrap();
    backend.push_key(keys::KEY_D, true, false).unwrap();
    assert_eq!(
        backend.push_key(keys::KEY_W, true, false),
        Err(BackendError::Overflow)
    );
    assert_eq!(
        backend.push_mouse_move(9.0, 9.0),
        Err(BackendError::Overflow)
    );
    assert_eq!(backend.dropped(), 2);
    assert_eq!(
        events(&mut backend),
        vec![
            InputEvent::key(keys::KEY_A, true),
            InputEvent::key(keys::KEY_D, true)
        ],
        "the dropped events are gone; the buffered ones survive"
    );

    // A drained buffer accepts again, and the counter keeps its history.
    backend.push_mouse_move(1.0, 1.0).unwrap();
    assert_eq!(events(&mut backend), vec![InputEvent::mouse_move(1.0, 1.0)]);
    assert_eq!(backend.dropped(), 2);
}

#[test]
fn partial_drains_leave_the_rest_queued() {
    // This test exercises the raw drain call itself, with a handle the test
    // owns and frees; the RAII wrapper always sizes its buffer to the full
    // count, so this path is the module's own guarantee, not the wrapper's.
    // SAFETY: `create` takes no preconditions; the handle is destroyed below
    // exactly once, and every call sees exclusive, live state.
    unsafe {
        let raw = mikan_input_sys::mikan_input_backend_create(8);
        assert!(!raw.is_null());
        assert_eq!(mikan_input_backend_push_key(raw, keys::KEY_A, 1, 0), OK);
        assert_eq!(mikan_input_backend_push_key(raw, keys::KEY_D, 1, 0), OK);
        assert_eq!(mikan_input_backend_push_key(raw, keys::KEY_A, 0, 0), OK);
        let mut out = [InputEvent::key(0, false); 2];
        let mut count = 0_usize;
        assert_eq!(
            mikan_input_backend_read(raw, out.as_mut_ptr(), 2, &mut count),
            OK
        );
        assert_eq!(count, 2);
        assert_eq!(out[0], InputEvent::key(keys::KEY_A, true));
        assert_eq!(out[1], InputEvent::key(keys::KEY_D, true));
        assert_eq!(mikan_input_backend_len(raw), 1, "the release stays queued");
        mikan_input_backend_destroy(raw);
    }
}

#[test]
fn clear_drops_buffered_events_but_keeps_device_state() {
    let mut backend = Backend::new(0);
    backend.push_key(keys::KEY_A, true, false).unwrap();
    backend.clear().unwrap();
    assert!(backend.is_empty());
    assert!(
        backend.is_key_down(keys::KEY_A),
        "clearing the buffer is not a release"
    );
}

#[test]
fn the_module_defends_itself_against_null_handles() {
    // These calls violate the raw declarations' preconditions on purpose;
    // the C side must answer with a status code instead of crashing.
    // SAFETY: null handles are explicitly tolerated by the module's
    // defensive contract; the calls read and write only stack locals.
    unsafe {
        let mut count = 0_usize;
        assert_eq!(
            mikan_input_backend_push_key(core::ptr::null_mut(), 1, 0, 0),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_input_backend_read(core::ptr::null_mut(), core::ptr::null_mut(), 0, &mut count),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_input_backend_clear(core::ptr::null_mut()),
            BAD_ARGUMENT
        );
        assert_eq!(
            mikan_input_backend_is_key_down(core::ptr::null(), keys::KEY_A),
            0
        );
        assert_eq!(
            mikan_input_backend_is_mouse_button_down(core::ptr::null(), 1),
            0
        );
        assert_eq!(mikan_input_backend_len(core::ptr::null()), 0);
        assert_eq!(mikan_input_backend_dropped(core::ptr::null()), 0);
    }
}

#[test]
fn backends_are_independent() {
    let mut first = Backend::new(0);
    let mut second = Backend::new(0);
    first.push_key(keys::KEY_A, true, false).unwrap();
    second.push_key(keys::KEY_D, true, false).unwrap();
    assert!(first.is_key_down(keys::KEY_A) && !second.is_key_down(keys::KEY_A));
    assert!(second.is_key_down(keys::KEY_D) && !first.is_key_down(keys::KEY_D));
}
