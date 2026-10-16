//! Exercises the C kernel through the crate's own safe entry points:
//! geometry, the separation definition, and the defensive input validation
//! the header promises.

use mikan_collision_sys::{Aabb, Contact, Pair, contacts, mikan_contacts};

fn only_contact(a: Aabb, b: Aabb) -> Contact {
    let bounds = [a, b];
    let pairs = [Pair {
        first: 0,
        second: 1,
    }];
    let mut out = [Contact {
        normal: [0.0, 0.0],
        depth: 0.0,
    }];
    contacts(&bounds, &pairs, &mut out);
    out[0]
}

#[test]
fn overlap_reports_depth_and_axis_normal() {
    let a = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
    let b = Aabb::from_center_size([6.0, 0.0], [10.0, 10.0]);
    let contact = only_contact(a, b);
    assert_eq!(contact.normal, [1.0, 0.0], "b lies to the right of a");
    assert_eq!(contact.depth, 4.0);

    let contact = only_contact(b, a);
    assert_eq!(
        contact.normal,
        [-1.0, 0.0],
        "the normal flips with the pair"
    );
    assert_eq!(contact.depth, 4.0);
}

#[test]
fn overlapping_on_both_axes_resolves_along_the_shallow_one() {
    let a = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
    let b = Aabb::from_center_size([3.0, -4.0], [10.0, 10.0]);
    let contact = only_contact(a, b);
    assert_eq!(contact.normal, [0.0, -1.0], "b is above a");
    assert_eq!(contact.depth, 6.0, "the shallow axis wins");
}

#[test]
fn touching_edges_are_separated() {
    let a = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
    let b = Aabb::from_center_size([10.0, 0.0], [10.0, 10.0]);
    let contact = only_contact(a, b);
    assert_eq!(contact.depth, 0.0);
    assert_eq!(contact.normal, [0.0, 0.0]);
    assert!(!contact.is_overlap());
}

#[test]
fn containment_and_identical_boxes_overlap() {
    let outer = Aabb::from_center_size([0.0, 0.0], [100.0, 100.0]);
    let inner = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
    let contact = only_contact(outer, inner);
    assert_eq!(
        contact.normal,
        [1.0, 0.0],
        "identical centers push toward +x"
    );
    assert_eq!(contact.depth, 10.0);

    let identical = only_contact(outer, outer);
    assert_eq!(identical.depth, 100.0);
}

#[test]
fn degenerate_and_invalid_boxes_are_separated() {
    let point = Aabb::from_center_size([0.0, 0.0], [0.0, 0.0]);
    let sane = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
    assert_eq!(only_contact(point, sane).depth, 0.0);

    let nan = Aabb {
        min: [f32::NAN, 0.0],
        max: [f32::NAN, 10.0],
    };
    assert_eq!(only_contact(nan, sane).depth, 0.0, "NaN is separated");
}

#[test]
fn out_of_range_pairs_are_separated_and_never_read_past_the_slice() {
    let bounds = [Aabb::from_center_size([0.0, 0.0], [10.0, 10.0])];
    let pairs = [
        Pair {
            first: 0,
            second: 0,
        },
        Pair {
            first: 0,
            second: 7,
        },
        Pair {
            first: 9,
            second: 0,
        },
    ];
    let mut out = [Contact {
        normal: [0.0, 0.0],
        depth: 0.0,
    }; 3];

    // The safe entry point debug-asserts valid indices by contract, so the
    // kernel's own defensive validation is exercised through the raw binding
    // — the code path a C caller could reach.
    //
    // SAFETY: every pointer and length below is valid for its slice; `out`
    // does not alias `bounds` or `pairs`; the kernel validates the
    // out-of-range indices itself and writes exactly `pairs.len()` contacts.
    unsafe {
        mikan_contacts(
            bounds.as_ptr(),
            bounds.len(),
            pairs.as_ptr(),
            pairs.len(),
            out.as_mut_ptr(),
        );
    }

    assert_eq!(out[0].depth, 10.0, "a box overlaps itself");
    assert_eq!(
        out[1],
        Contact {
            normal: [0.0, 0.0],
            depth: 0.0
        }
    );
    assert_eq!(
        out[2],
        Contact {
            normal: [0.0, 0.0],
            depth: 0.0
        }
    );
}

#[test]
fn empty_inputs_are_no_ops() {
    let mut out: [Contact; 0] = [];
    contacts(&[], &[], &mut out);
}
