//! Times the C kernel against the Rust reference on the same pair set.
//!
//! This is the measurement behind `docs/STATUS.md`'s collision numbers and
//! the honest baseline ADR 0010 promises: same inputs, same output buffer
//! discipline, warm-up plus repeated runs, release profile. Run it through
//! `scripts/bench_collision.sh`, or directly:
//!
//!     cargo run -p mikan-collision --release --example bench [PAIR_COUNT]

use std::hint::black_box;
use std::time::{Duration, Instant};

use mikan_collision::{Aabb, Contact, Pair, contacts, reference};

struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
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

fn main() {
    let pair_count: usize = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(250_000);
    let box_count: usize = 512;
    let runs: u32 = 25;

    // A dense field of boxes with a deterministic random pair set: enough
    // real overlaps to time the overlap path, not only the separated one.
    let mut rng = Lcg(0x0123_4567_89ab_cdef);
    let bounds: Vec<Aabb> = (0..box_count)
        .map(|_| {
            let center = [
                rng.next_f32() * 800.0 - 400.0,
                rng.next_f32() * 800.0 - 400.0,
            ];
            let half = [rng.next_f32() * 60.0 + 2.0, rng.next_f32() * 60.0 + 2.0];
            Aabb::from_center_size(center, [half[0] * 2.0, half[1] * 2.0])
        })
        .collect();
    let pairs: Vec<Pair> = (0..pair_count)
        .map(|_| {
            let first = rng.next_u32() % box_count as u32;
            let second = if rng.next_u32().is_multiple_of(2) {
                rng.next_u32() % box_count as u32
            } else {
                (first + rng.next_u32() % 16) % box_count as u32
            };
            Pair { first, second }
        })
        .collect();

    let mut out_c = vec![
        Contact {
            normal: [0.0, 0.0],
            depth: 0.0,
        };
        pair_count
    ];
    let mut out_rust = out_c.clone();

    contacts(black_box(&bounds), black_box(&pairs), &mut out_c);
    reference::contacts(black_box(&bounds), black_box(&pairs), &mut out_rust);
    assert_eq!(
        out_c, out_rust,
        "the kernel and the reference must agree before any timing is trusted"
    );
    let overlaps = out_c.iter().filter(|contact| contact.is_overlap()).count();

    let c_time = time(runs, || {
        contacts(black_box(&bounds), black_box(&pairs), &mut out_c);
    });
    let rust_time = time(runs, || {
        reference::contacts(black_box(&bounds), black_box(&pairs), &mut out_rust);
    });

    let per_pair = |duration: Duration| duration.as_nanos() as f64 / pair_count as f64;
    println!(
        "pairs: {pair_count} over {box_count} boxes, {overlaps} overlapping, {runs} runs each"
    );
    println!(
        "c kernel:        {:.3} ms/run ({:.2} ns/pair)",
        c_time.as_secs_f64() * 1e3,
        per_pair(c_time)
    );
    println!(
        "rust reference:  {:.3} ms/run ({:.2} ns/pair)",
        rust_time.as_secs_f64() * 1e3,
        per_pair(rust_time)
    );
    println!(
        "ratio c/reference: {:.2} (below 1.0 means C is faster)",
        c_time.as_secs_f64() / rust_time.as_secs_f64()
    );
}
