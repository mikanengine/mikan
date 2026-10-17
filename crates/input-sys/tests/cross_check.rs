//! Cross-checks: the C backend's normalization against the independent Rust
//! reference, bit for bit.
//!
//! The reference (`mikan_input_sys::reference`) re-implements the C rules;
//! these tests fail loudly if the two drift. Fixed sequences cover the
//! documented rules; a randomized sequence drives both implementations
//! through thousands of operations and compares every drained batch and the
//! final device state.

use mikan_input_sys::{Backend, keys, reference::Normalizer};

/// A small deterministic LCG so the randomized test is reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 16
    }
}

const KEY_POOL: [u32; 6] = [
    keys::KEY_A,
    keys::KEY_D,
    keys::KEY_W,
    keys::KEY_ESCAPE,
    keys::KEY_ARROWLEFT,
    keys::KEY_SPACE,
];
const BUTTON_POOL: [u32; 3] = [
    mikan_input_sys::MOUSE_LEFT,
    mikan_input_sys::MOUSE_RIGHT,
    mikan_input_sys::MOUSE_MIDDLE,
];

/// Drives one random operation through both implementations.
fn drive(backend: &mut Backend, reference: &mut Normalizer, rng: &mut Lcg, op: u64) {
    match op % 8 {
        0 | 1 => {
            let code = KEY_POOL[(rng.next() % KEY_POOL.len() as u64) as usize];
            let pressed = rng.next().is_multiple_of(2);
            backend.push_key(code, pressed, false).unwrap();
            reference.push_key(code, pressed, false);
        }
        2 => {
            let code = KEY_POOL[(rng.next() % KEY_POOL.len() as u64) as usize];
            backend.push_key(code, true, true).unwrap();
            reference.push_key(code, true, true);
        }
        3 | 4 => {
            let code = BUTTON_POOL[(rng.next() % BUTTON_POOL.len() as u64) as usize];
            let pressed = rng.next().is_multiple_of(2);
            backend.push_mouse_button(code, pressed).unwrap();
            reference.push_mouse_button(code, pressed);
        }
        5 => {
            let x = (rng.next() % 4096) as f64;
            let y = (rng.next() % 4096) as f64;
            backend.push_mouse_move(x, y).unwrap();
            reference.push_mouse_move(x, y);
        }
        6 => {
            let dx = ((rng.next() % 7) as f64) - 3.0;
            let dy = ((rng.next() % 7) as f64) - 3.0;
            backend.push_wheel(dx, dy).unwrap();
            reference.push_wheel(dx, dy);
        }
        _ => {
            let focused = rng.next().is_multiple_of(2);
            backend.push_focus(focused).unwrap();
            reference.push_focus(focused);
        }
    }
}

fn drain_and_compare(backend: &mut Backend, reference: &mut Normalizer) {
    let mut native = Vec::new();
    backend.drain(&mut native).unwrap();
    let oracle = reference.take();
    assert_eq!(
        native, oracle,
        "the C backend and the Rust reference disagree"
    );
}

#[test]
fn fixed_sequences_agree() {
    let mut backend = Backend::new(0);
    let mut reference = Normalizer::new();

    // Duplicates, spurious releases, repeats.
    backend.push_key(keys::KEY_A, true, false).unwrap();
    backend.push_key(keys::KEY_A, true, false).unwrap();
    backend.push_key(keys::KEY_A, true, true).unwrap();
    backend.push_key(keys::KEY_A, false, false).unwrap();
    backend.push_key(keys::KEY_A, false, false).unwrap();
    reference.push_key(keys::KEY_A, true, false);
    reference.push_key(keys::KEY_A, true, false);
    reference.push_key(keys::KEY_A, true, true);
    reference.push_key(keys::KEY_A, false, false);
    reference.push_key(keys::KEY_A, false, false);
    drain_and_compare(&mut backend, &mut reference);

    // Coalescing and the zero-wheel drop.
    backend.push_wheel(1.0, 2.0).unwrap();
    backend.push_wheel(0.5, -1.0).unwrap();
    backend.push_wheel(0.0, 0.0).unwrap();
    backend.push_mouse_move(10.0, 10.0).unwrap();
    backend.push_mouse_move(20.0, 30.0).unwrap();
    reference.push_wheel(1.0, 2.0);
    reference.push_wheel(0.5, -1.0);
    reference.push_wheel(0.0, 0.0);
    reference.push_mouse_move(10.0, 10.0);
    reference.push_mouse_move(20.0, 30.0);
    drain_and_compare(&mut backend, &mut reference);

    // Focus loss with held devices, then re-gain.
    backend.push_key(keys::KEY_W, true, false).unwrap();
    backend
        .push_mouse_button(mikan_input_sys::MOUSE_MIDDLE, true)
        .unwrap();
    backend.push_focus(false).unwrap();
    backend.push_focus(false).unwrap();
    backend.push_focus(true).unwrap();
    reference.push_key(keys::KEY_W, true, false);
    reference.push_mouse_button(mikan_input_sys::MOUSE_MIDDLE, true);
    reference.push_focus(false);
    reference.push_focus(false);
    reference.push_focus(true);
    drain_and_compare(&mut backend, &mut reference);

    assert_eq!(reference.events(), &[], "every event was drained");
}

#[test]
fn randomized_sequences_agree() {
    let mut rng = Lcg(0x5EED_CAFE);
    let mut backend = Backend::new(0);
    let mut reference = Normalizer::new();

    for operation in 0..10_000 {
        drive(&mut backend, &mut reference, &mut rng, operation);
        // Compare every 64 operations and at the end.
        if operation % 64 == 63 {
            drain_and_compare(&mut backend, &mut reference);
        }
    }
    drain_and_compare(&mut backend, &mut reference);

    // The final raw device state agrees too, for the whole key pool.
    for &code in &KEY_POOL {
        assert_eq!(
            backend.is_key_down(code),
            reference.is_key_down(code),
            "key {code:#04x} state disagrees after the run"
        );
    }
    for &code in &BUTTON_POOL {
        assert_eq!(
            backend.is_mouse_button_down(code),
            reference.is_mouse_button_down(code),
            "button {code} state disagrees after the run"
        );
    }
    assert!(backend.dropped() == 0, "the test buffer never overflows");
}
