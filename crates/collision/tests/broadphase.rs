//! Broad-phase behavior: candidates are complete for overlapping pairs,
//! deduplicated, and deterministic across rebuilds.

use mikan_collision::{Aabb, Broadphase, Pair};

fn pairs_of(bounds: &[Aabb], cell: f32) -> Vec<Pair> {
    let mut broadphase = Broadphase::new(cell);
    broadphase.rebuild(bounds);
    let mut pairs = Vec::new();
    broadphase.pairs(&mut pairs);
    pairs
}

#[test]
fn finds_a_near_pair_once() {
    let bounds = [
        Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]),
        Aabb::from_center_size([6.0, 0.0], [10.0, 10.0]),
    ];
    assert_eq!(
        pairs_of(&bounds, 16.0),
        vec![Pair {
            first: 0,
            second: 1
        }]
    );
}

#[test]
fn distant_boxes_are_not_candidates() {
    let bounds = [
        Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]),
        Aabb::from_center_size([1000.0, 1000.0], [10.0, 10.0]),
    ];
    assert!(pairs_of(&bounds, 16.0).is_empty());
}

#[test]
fn boxes_spanning_many_cells_yield_one_pair() {
    let bounds = [
        Aabb::from_center_size([0.0, 0.0], [200.0, 200.0]),
        Aabb::from_center_size([50.0, -50.0], [200.0, 200.0]),
    ];
    assert_eq!(
        pairs_of(&bounds, 16.0),
        vec![Pair {
            first: 0,
            second: 1
        }],
        "the pair is emitted once even though the boxes share dozens of cells"
    );
}

#[test]
fn touching_cell_boundaries_are_candidates() {
    let bounds = [
        Aabb::from_center_size([5.0, 5.0], [10.0, 10.0]),
        Aabb::from_center_size([15.0, 5.0], [10.0, 10.0]),
    ];
    assert_eq!(
        pairs_of(&bounds, 10.0),
        vec![Pair {
            first: 0,
            second: 1
        }],
        "edge-adjacent boxes share the boundary cell; the narrow phase decides"
    );
}

#[test]
fn output_is_deterministic_across_rebuilds() {
    let bounds: Vec<Aabb> = (0..64)
        .map(|index| {
            let x = (index % 8) as f32 * 12.0;
            let y = (index / 8) as f32 * 12.0;
            Aabb::from_center_size([x, y], [16.0, 16.0])
        })
        .collect();
    let mut broadphase = Broadphase::new(12.0);
    let mut first = Vec::new();
    let mut second = Vec::new();
    broadphase.rebuild(&bounds);
    broadphase.pairs(&mut first);
    broadphase.rebuild(&bounds);
    broadphase.pairs(&mut second);
    assert_eq!(first, second);
    assert!(first.windows(2).all(|window| window[0] < window[1]));
}

#[test]
fn degenerate_and_invalid_boxes_are_skipped() {
    let bounds = [
        Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]),
        Aabb::from_center_size([0.0, 0.0], [0.0, 0.0]),
        Aabb {
            min: [5.0, 5.0],
            max: [-5.0, -5.0],
        },
        Aabb {
            min: [f32::NAN, 0.0],
            max: [f32::NAN, 10.0],
        },
    ];
    let pairs = pairs_of(&bounds, 16.0);
    assert!(
        pairs.iter().all(|pair| pair.first < 1 && pair.second < 1),
        "only the first, valid box may appear in candidates: {pairs:?}"
    );
}
