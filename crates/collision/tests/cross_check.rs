//! Cross-checks the C kernel against the Rust reference implementation.
//!
//! This is the test that keeps the two implementations honest (ADR 0010):
//! every scenario — fixed cases, near-edge offsets, and randomized pair
//! sets — runs through both, and the results are compared exactly. Floating
//! point operations here are only additions, subtractions, and comparisons
//! in identical order, so the two paths are expected to agree bit for bit.

use mikan_collision::{Aabb, Contact, Pair, contacts, reference};

fn cross_check(bounds: &[Aabb], pairs: &[Pair]) {
    let mut from_c = vec![
        Contact {
            normal: [0.0, 0.0],
            depth: 0.0,
        };
        pairs.len()
    ];
    let mut from_rust = from_c.clone();
    contacts(bounds, pairs, &mut from_c);
    reference::contacts(bounds, pairs, &mut from_rust);
    for (index, (c, rust)) in from_c.iter().zip(&from_rust).enumerate() {
        assert_eq!(
            c, rust,
            "pair {index} ({:?}) disagrees between the C kernel and the Rust reference",
            pairs[index]
        );
    }
}

/// Boxes around the origin whose relative offsets sweep an edge and a corner.
fn edge_cases() -> Vec<Aabb> {
    let mut boxes = vec![Aabb::from_center_size([0.0, 0.0], [10.0, 10.0])];
    for offset in [
        -20.0, -10.0, -9.999, -5.0, -0.001, 0.0, 0.001, 5.0, 9.999, 10.0, 20.0,
    ] {
        boxes.push(Aabb::from_center_size([offset, offset], [10.0, 10.0]));
        boxes.push(Aabb::from_center_size([offset, 0.0], [10.0, 10.0]));
        boxes.push(Aabb::from_center_size([0.0, offset], [10.0, 10.0]));
    }
    boxes.push(Aabb::from_center_size([100.0, 100.0], [0.0, 0.0]));
    boxes.push(Aabb {
        min: [f32::NAN, 0.0],
        max: [f32::NAN, 10.0],
    });
    boxes.push(Aabb {
        min: [5.0, 5.0],
        max: [-5.0, -5.0],
    });
    boxes
}

#[test]
fn fixed_edge_cases_agree() {
    let bounds = edge_cases();
    let mut pairs = Vec::new();
    for first in 0..bounds.len() {
        for second in 0..bounds.len() {
            pairs.push(Pair {
                first: first as u32,
                second: second as u32,
            });
        }
    }
    cross_check(&bounds, &pairs);
}

#[test]
fn randomized_pair_sets_agree() {
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

    let mut rng = Lcg(0x5eed_0010_c0ff_ee01);
    let bounds: Vec<Aabb> = (0..256)
        .map(|_| {
            let center = [
                rng.next_f32() * 400.0 - 200.0,
                rng.next_f32() * 400.0 - 200.0,
            ];
            let size = [rng.next_f32() * 60.0 + 1.0, rng.next_f32() * 60.0 + 1.0];
            Aabb::from_center_size(center, size)
        })
        .collect();

    for _ in 0..8 {
        let pairs: Vec<Pair> = (0..20_000)
            .map(|_| Pair {
                first: rng.next_u32() % bounds.len() as u32,
                second: rng.next_u32() % bounds.len() as u32,
            })
            .collect();
        cross_check(&bounds, &pairs);
    }
}

#[test]
fn overlapping_density_is_meaningful() {
    // Guards against a benchmark that times almost nothing: on the same
    // generator shape, a healthy fraction of pairs must actually overlap.
    let bounds: Vec<Aabb> = (0..512)
        .map(|index| {
            let x = (index % 32) as f32 * 25.0;
            let y = (index / 32) as f32 * 25.0;
            Aabb::from_center_size([x, y], [40.0, 40.0])
        })
        .collect();
    let pairs: Vec<Pair> = (0..10_000_u32)
        .map(|index| {
            let first = index % 512;
            let step = if index.is_multiple_of(2) { 1 } else { 32 };
            Pair {
                first,
                second: (first + step) % 512,
            }
        })
        .collect();
    let mut out = vec![
        Contact {
            normal: [0.0, 0.0],
            depth: 0.0,
        };
        pairs.len()
    ];
    contacts(&bounds, &pairs, &mut out);
    let overlaps = out.iter().filter(|contact| contact.is_overlap()).count();
    assert!(
        overlaps > pairs.len() / 2,
        "expected a meaningful overlap rate, got {overlaps} of {}",
        pairs.len()
    );
}
