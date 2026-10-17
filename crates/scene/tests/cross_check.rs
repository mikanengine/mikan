//! Cross-checks the native scene module against the independent Rust
//! reference, bit for bit, and the graph's propagation against a
//! `reference::compose` fold over the same inputs.
//!
//! "Bit for bit" is the actual requirement: the kernel promises a fixed
//! grouping of every multiply and add (see `mikan_transform.h`), so the two
//! implementations must round identically, not just approximately. `f32 ==`
//! cannot express that (`-0.0 == 0.0`, and NaN never equals), so every
//! comparison goes through `to_bits`.

use mikan_ecs::{Entity, World};
use mikan_scene::reference::{bits, compose as reference_compose};
use mikan_scene::{GlobalTransform2d, Hierarchy, Transform2d, propagate_transforms};
use mikan_scene_sys::{Transform2dRepr, compose};

/// A table of values chosen to stress rounding: exact powers of two, values
/// that are inexact in binary, huge and tiny magnitudes, negative and zero
/// scales, and rotations where `sin_cos` is inexact.
fn table() -> Vec<Transform2dRepr> {
    let translations = [
        [0.0, 0.0],
        [1.0, -1.0],
        [0.1, -0.7],
        [1234.5678, -9876.543],
        [1.0e6, -1.0e6],
        [1.0e-6, 3.0e-7],
        [f32::MIN_POSITIVE, -f32::MIN_POSITIVE],
        [f32::MAX / 4.0, -f32::MAX / 4.0],
    ];
    let rotations = [
        0.0,
        core::f32::consts::FRAC_PI_4,
        core::f32::consts::FRAC_PI_2,
        core::f32::consts::PI,
        -core::f32::consts::PI,
        1.0,
        -3.75,
        100.0,
    ];
    let scales = [
        [1.0, 1.0],
        [-1.0, 1.0],
        [0.0, 0.0],
        [0.5, 2.0],
        [1.0e-20, 12345.678],
        [-7.25, -0.125],
    ];
    let mut out = Vec::new();
    for translation in translations {
        for rotation in rotations {
            for scale in scales {
                out.push(Transform2d::from_parts(translation, rotation, scale).to_repr());
            }
        }
    }
    out
}

#[test]
fn by_value_compose_matches_the_reference_bit_for_bit() {
    let inputs = table();
    for parent in &inputs {
        for local in &inputs {
            let native = compose(parent, local);
            let rust = reference_compose(parent, local);
            assert_eq!(
                bits(&native),
                bits(&rust),
                "parent {parent:?} with local {local:?}"
            );
        }
    }
}

/// A deterministic xorshift generator so the random sweep is reproducible on
/// every machine for a given binary (the same promise the engine makes).
struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// A float with a mixed exponent, sign, and mantissa.
    fn next_f32(&mut self) -> f32 {
        let raw = self.next_u32();
        f32::from_bits(raw & !0x7f80_0000 | (raw & 0x3f80_0000))
    }

    fn next_transform(&mut self) -> Transform2dRepr {
        Transform2d::from_parts(
            [self.next_f32(), self.next_f32()],
            self.next_f32(),
            [self.next_f32(), self.next_f32()],
        )
        .to_repr()
    }
}

#[test]
fn by_value_compose_matches_the_reference_on_random_inputs() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..5_000 {
        let parent = rng.next_transform();
        let local = rng.next_transform();
        let native = compose(&parent, &local);
        let rust = reference_compose(&parent, &local);
        assert_eq!(bits(&native), bits(&rust), "{parent:?} with {local:?}");
    }
}

#[test]
fn rotation_composes_by_exact_addition_and_identity_is_exact() {
    let parent = Transform2d::from_parts([0.0, 0.0], 1.0, [1.0, 1.0]).to_repr();
    let local = Transform2d::from_parts([0.0, 0.0], 2.0, [1.0, 1.0]).to_repr();
    assert_eq!(compose(&parent, &local).rotation, 3.0);
    assert_eq!(compose(&parent, &local), reference_compose(&parent, &local));
    assert_eq!(compose(&Transform2dRepr::identity(), &local), local);
    assert_eq!(compose(&parent, &Transform2dRepr::identity()), parent);
}

/// Assigns local transforms to a three-deep chain with a branch, propagates
/// through the native graph, and compares every resulting world transform
/// against a `reference::compose` fold over the ancestry.
#[test]
fn graph_propagation_matches_a_reference_fold() {
    let mut world = World::new();
    world.insert_resource(Hierarchy::new());

    let root = world.spawn();
    let middle = world.spawn();
    let leaf = world.spawn();
    let branch = world.spawn();
    let locals = [
        Transform2d::from_parts([1.5, -2.5], 0.3, [2.0, 0.5]),
        Transform2d::from_parts([-7.25, 0.125], -1.2, [1.0, 3.0]),
        Transform2d::from_parts([0.0, 12.0], core::f32::consts::FRAC_PI_2, [0.5, 0.5]),
        Transform2d::from_parts([3.0, 4.0], 100.0, [-1.0, 2.0]),
    ];
    for (entity, local) in [(root, 0), (middle, 1), (leaf, 2), (branch, 3)] {
        world.insert(entity, locals[local]).unwrap();
    }
    {
        let mut hierarchy = world.remove_resource::<Hierarchy>().unwrap();
        for entity in [leaf, branch, middle, root] {
            hierarchy.add(entity).unwrap();
        }
        hierarchy.set_parent(middle, root).unwrap();
        hierarchy.set_parent(leaf, middle).unwrap();
        hierarchy.set_parent(branch, root).unwrap();
        world.insert_resource(hierarchy);
    }

    propagate_transforms(&mut world);

    // Expected worlds, folded by hand through the independent reference.
    let root_repr = locals[0].to_repr();
    let middle_repr = reference_compose(&root_repr, &locals[1].to_repr());
    let leaf_repr = reference_compose(&middle_repr, &locals[2].to_repr());
    let branch_repr = reference_compose(&root_repr, &locals[3].to_repr());

    let check = |entity: Entity, expected: &Transform2dRepr, name: &str| {
        let global = world
            .get::<GlobalTransform2d>(entity)
            .unwrap_or_else(|| panic!("{name} must have a world transform"))
            .to_transform();
        assert_eq!(
            global.translation[0].to_bits(),
            expected.translation_x.to_bits(),
            "{name} x"
        );
        assert_eq!(
            global.translation[1].to_bits(),
            expected.translation_y.to_bits(),
            "{name} y"
        );
        assert_eq!(
            global.rotation.to_bits(),
            expected.rotation.to_bits(),
            "{name} rotation"
        );
        assert_eq!(
            global.scale[0].to_bits(),
            expected.scale_x.to_bits(),
            "{name} scale x"
        );
        assert_eq!(
            global.scale[1].to_bits(),
            expected.scale_y.to_bits(),
            "{name} scale y"
        );
    };

    check(root, &root_repr, "root");
    check(middle, &middle_repr, "middle");
    check(leaf, &leaf_repr, "leaf");
    check(branch, &branch_repr, "branch");
}
