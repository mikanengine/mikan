//! Times the native input pipeline against its Rust reference.
//!
//! The measurement behind `docs/input.md` and `docs/STATUS.md`, in the spirit
//! of `crates/collision/examples/bench.rs`: release mode, warm-up, repeated
//! runs over the same deterministic batch stream, and a bit-for-bit parity
//! check that both kernels normalize the same input to the same output
//! before any number is printed.
//!
//! Three timings:
//!
//! 1. **engine step** — the full path a game runs per fixed update:
//!    [`InputSystem::process`] (translation + C backend normalization + C++
//!    action runtime), queries, and `end_step`.
//! 2. **kernel** — the C backend alone on the same raw push stream.
//! 3. **reference** — the Rust `Normalizer` oracle on that stream.
//!
//! Run it through `scripts/bench_input.sh`, or directly:
//!
//!     cargo run -p mikan-input --release --example bench_input [BATCHES]

use std::hint::black_box;
use std::time::{Duration, Instant};

use mikan_input::{ActionId, Binding, InputSystem, WheelAxis};
use mikan_input_sys as inf;
use mikan_input_sys::reference::Normalizer;
use mikan_platform::DeviceEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn next_f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn next_pos(&mut self) -> (f64, f64) {
        (
            self.next_f64() * 1_500.0 - 200.0,
            self.next_f64() * 1_200.0 - 100.0,
        )
    }
}

/// The raw batch shape: one fixed-update worth of device traffic.
enum Raw {
    /// A key transition.
    Key { code: u32, pressed: bool, repeat: bool },
    /// An absolute cursor position.
    Move { x: f64, y: f64 },
    /// One wheel step.
    Wheel { dx: f64, dy: f64 },
}

/// 6 key transitions, 2 cursor moves, 1 wheel tick.
fn make_raw_batch(lcg: &mut Lcg) -> Vec<Raw> {
    let (x1, y1) = lcg.next_pos();
    let (x2, y2) = lcg.next_pos();
    vec![
        Raw::Key { code: inf::keys::KEY_G, pressed: true, repeat: false },
        Raw::Key { code: inf::keys::KEY_H, pressed: false, repeat: false },
        Raw::Key { code: inf::keys::KEY_G, pressed: false, repeat: false },
        Raw::Key { code: inf::keys::KEY_J, pressed: true, repeat: false },
        Raw::Key { code: inf::keys::KEY_K, pressed: false, repeat: false },
        Raw::Key { code: inf::keys::KEY_L, pressed: true, repeat: false },
        Raw::Move { x: x1, y: y1 },
        Raw::Move { x: x2, y: y2 },
        Raw::Wheel { dx: 3.0, dy: -8.5 },
    ]
}

/// The same shape as platform events for the engine-step section.
fn make_device_batch(lcg: &mut Lcg) -> Vec<DeviceEvent> {
    let (x1, y1) = lcg.next_pos();
    let (x2, y2) = lcg.next_pos();
    let key_event = |code: KeyCode, pressed: bool| {
        DeviceEvent::Key {
            key: PhysicalKey::Code(code),
            pressed,
            repeat: false,
        }
    };
    vec![
        key_event(KeyCode::KeyG, true),
        key_event(KeyCode::KeyH, false),
        key_event(KeyCode::KeyG, false),
        key_event(KeyCode::KeyJ, true),
        key_event(KeyCode::KeyK, false),
        key_event(KeyCode::KeyL, true),
        DeviceEvent::CursorMoved {
            position: winit::dpi::PhysicalPosition::new(x1, y1),
        },
        DeviceEvent::CursorMoved {
            position: winit::dpi::PhysicalPosition::new(x2, y2),
        },
        DeviceEvent::MouseWheel { delta: (3.0, -8.5) },
    ]
}

fn time(runs: u32, mut body: impl FnMut()) -> Duration {
    for _ in 0..3 {
        body();
    }
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed() / runs
}

fn report(label: &str, mean: Duration, batches: usize, events_per_batch: usize) {
    let per_batch = mean.as_nanos() as f64 / batches as f64;
    let per_event = per_batch / events_per_batch as f64;
    println!(
        "{label:<12} {per_batch:>9.1} ns/batch | {per_event:>6.2} ns/event | {:>10.0} batches/s",
        1.0e9 / per_batch.max(1.0),
    );
}

fn main() {
    let batches: usize = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(65_536);
    let runs: u32 = 40;

    let mut lcg = Lcg::new(0x5EED_1A17);
    let raw_stream: Vec<Vec<Raw>> =
        (0..batches).map(|_| make_raw_batch(&mut lcg)).collect();
    let mut lcg = Lcg::new(0x5EED_1A17);
    let device_stream: Vec<Vec<DeviceEvent>> =
        (0..batches).map(|_| make_device_batch(&mut lcg)).collect();
    let events_per_batch = raw_stream[0].len();

    // ---- parity gate: the two kernels must agree, batch by batch ----
    {
        let mut backend = inf::Backend::new(0);
        let mut oracle = Normalizer::new();
        let mut native_batch = Vec::new();
        for batch in &raw_stream {
            for raw in batch {
                match *raw {
                    Raw::Key { code, pressed, repeat } => {
                        backend
                            .push_key(code, pressed, repeat)
                            .expect("a bench batch always fits");
                        oracle.push_key(code, pressed, repeat);
                    }
                    Raw::Move { x, y } => {
                        backend
                            .push_mouse_move(x, y)
                            .expect("a bench batch always fits");
                        oracle.push_mouse_move(x, y);
                    }
                    Raw::Wheel { dx, dy } => {
                        backend
                            .push_wheel(dx, dy)
                            .expect("a bench batch always fits");
                        oracle.push_wheel(dx, dy);
                    }
                }
            }
            native_batch.clear();
            backend
                .drain(&mut native_batch)
                .expect("drain cannot fail");
            assert_eq!(
                native_batch,
                oracle.take(),
                "the two normalizations must agree before timings are printed"
            );
        }
    }

    // ---- section 2: kernel and reference, same state across batches ----
    let mut backend = inf::Backend::new(0);
    let mut scratch: Vec<inf::InputEvent> = Vec::new();
    let native_mean = time(runs, || {
        for batch in &raw_stream {
            for raw in batch {
                match *raw {
                    Raw::Key { code, pressed, repeat } => backend
                        .push_key(code, pressed, repeat)
                        .expect("a bench batch always fits"),
                    Raw::Move { x, y } => backend
                        .push_mouse_move(x, y)
                        .expect("a bench batch always fits"),
                    Raw::Wheel { dx, dy } => backend
                        .push_wheel(dx, dy)
                        .expect("a bench batch always fits"),
                };
            }
            backend.drain(&mut scratch).expect("drain cannot fail");
            black_box(scratch.last());
        }
    });

    let mut oracle = Normalizer::new();
    let reference_mean = time(runs, || {
        for batch in &raw_stream {
            for raw in batch {
                match *raw {
                    Raw::Key { code, pressed, repeat } => {
                        oracle.push_key(code, pressed, repeat);
                    }
                    Raw::Move { x, y } => oracle.push_mouse_move(x, y),
                    Raw::Wheel { dx, dy } => oracle.push_wheel(dx, dy),
                }
            }
            black_box(oracle.take());
        }
    });

    // ---- section 1: the full engine step ----
    let mut system = InputSystem::new();
    let gameplay = system.create_context("gameplay", 0);
    const HOLD_G: ActionId = ActionId::new(1);
    const FIRE_J: ActionId = ActionId::new(2);
    const HOLD_L: ActionId = ActionId::new(3);
    const SCROLL: ActionId = ActionId::new(4);
    for (binding, action) in [
        (Binding::Key(PhysicalKey::Code(KeyCode::KeyG)), HOLD_G),
        (Binding::Key(PhysicalKey::Code(KeyCode::KeyJ)), FIRE_J),
        (Binding::Key(PhysicalKey::Code(KeyCode::KeyL)), HOLD_L),
        (
            Binding::Wheel(WheelAxis::Vertical),
            SCROLL,
        ),
    ] {
        system
            .bind(gameplay, binding, action)
            .expect("every bench binding is a stable code");
    }
    let engine_mean = time(runs, || {
        for batch in &device_stream {
            system
                .process(batch)
                .expect("a bench stream never overflows");
            black_box((
                system.held(HOLD_G),
                system.just_pressed(FIRE_J),
                system.just_released(FIRE_J),
                system.value(SCROLL),
            ));
            system.end_step();
        }
    });

    println!(
        "input bench: {batches} batches x {events_per_batch} events, {runs} runs"
    );
    report("engine step", engine_mean, batches, events_per_batch);
    report("kernel", native_mean, batches, events_per_batch);
    report("reference", reference_mean, batches, events_per_batch);
}
