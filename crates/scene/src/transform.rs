//! The two transform components: local state and the derived world result.

use mikan_scene_sys::Transform2dRepr;

/// An entity's local transform: placed by gameplay, relative to its parent.
///
/// The three parts compose in the usual 2D order — the child's translation is
/// scaled and rotated by the parent, then the parent's translation is added —
/// and rotation is in radians. Scale components of `1.0` and rotation `0.0`
/// are the identity; there is no required normalization, so a zero or
/// negative scale is legal and simply produces a degenerate (mirrored or
/// collapsed) world transform, exactly as the arithmetic says.
///
/// This is simulation state: systems mutate it through the world, and
/// [`crate::propagate_transforms`] reads it. It is never written by the
/// engine itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2d {
    /// Position relative to the parent (or the world origin for a root).
    pub translation: [f32; 2],
    /// Rotation in radians, relative to the parent.
    pub rotation: f32,
    /// Scale relative to the parent.
    pub scale: [f32; 2],
}

impl Transform2d {
    /// The identity transform: origin, no rotation, unit scale.
    pub const IDENTITY: Self = Self {
        translation: [0.0, 0.0],
        rotation: 0.0,
        scale: [1.0, 1.0],
    };

    /// A transform that only translates.
    #[must_use]
    pub const fn from_translation(translation: [f32; 2]) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// Builds a transform from its parts.
    #[must_use]
    pub const fn from_parts(translation: [f32; 2], rotation: f32, scale: [f32; 2]) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Converts to the ABI form, computing the cached cosine/sine pair once.
    ///
    /// The native kernel never calls trigonometry: it receives the pair and
    /// keeps it exact through composition. Computing it here (per local
    /// transform per propagation, not per edge) is what makes a deep
    /// hierarchy cost one `sin_cos` per node per step.
    #[must_use]
    pub fn to_repr(self) -> Transform2dRepr {
        let (sin_rotation, cos_rotation) = self.rotation.sin_cos();
        Transform2dRepr {
            translation_x: self.translation[0],
            translation_y: self.translation[1],
            rotation: self.rotation,
            scale_x: self.scale[0],
            scale_y: self.scale[1],
            cos_rotation,
            sin_rotation,
        }
    }
}

impl Default for Transform2d {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// An entity's world-space transform, derived from its ancestry.
///
/// [`crate::propagate_transforms`] recomputes this component from scratch for
/// every hierarchy entity on every step, composing the entity's
/// [`Transform2d`] with its parent's `GlobalTransform2d`. It is a cache:
/// mutating it directly has no lasting effect, and it must not be treated as
/// simulation state.
///
/// The rotation's cosine/sine pair is kept as composed, so reading a world
/// rotation never re-rounds through trigonometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform2d(Transform2dRepr);

impl GlobalTransform2d {
    /// The identity world transform, for entities that were never propagated
    /// or hold no local transform.
    pub const IDENTITY: Self = Self(Transform2dRepr::identity());

    /// The world-space translation.
    #[must_use]
    pub const fn translation(&self) -> [f32; 2] {
        [self.0.translation_x, self.0.translation_y]
    }

    /// The accumulated world rotation, in radians.
    #[must_use]
    pub const fn rotation(&self) -> f32 {
        self.0.rotation
    }

    /// The accumulated world scale.
    #[must_use]
    pub const fn scale(&self) -> [f32; 2] {
        [self.0.scale_x, self.0.scale_y]
    }

    /// Decomposes the world transform into a [`Transform2d`]-shaped value.
    ///
    /// This is the shape a bake or a manual world-space operation needs; the
    /// cached cosine/sine pair is dropped, because a [`Transform2d`] recomputes
    /// it when it is next converted to the ABI form.
    #[must_use]
    pub const fn to_transform(&self) -> Transform2d {
        Transform2d {
            translation: self.translation(),
            rotation: self.0.rotation,
            scale: self.scale(),
        }
    }

    /// Wraps a propagated ABI value.
    pub(crate) const fn from_repr(repr: Transform2dRepr) -> Self {
        Self(repr)
    }
}

impl Default for GlobalTransform2d {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    #[test]
    fn identity_repr_is_exact() {
        let repr = Transform2d::IDENTITY.to_repr();
        assert_eq!(repr, Transform2dRepr::identity());
    }

    #[test]
    fn to_repr_caches_the_unit_pair() {
        let repr = Transform2d::from_parts([1.0, 2.0], FRAC_PI_2, [2.0, 3.0]).to_repr();
        assert_eq!(repr.translation_x, 1.0);
        assert_eq!(repr.translation_y, 2.0);
        assert_eq!(repr.rotation, FRAC_PI_2);
        assert_eq!(repr.scale_x, 2.0);
        assert_eq!(repr.scale_y, 3.0);
        let (sin, cos) = FRAC_PI_2.sin_cos();
        assert_eq!(repr.cos_rotation, cos);
        assert_eq!(repr.sin_rotation, sin);
        let length = repr.cos_rotation * repr.cos_rotation + repr.sin_rotation * repr.sin_rotation;
        assert!((length - 1.0).abs() < 1e-6);
    }

    #[test]
    fn global_transform_round_trips_through_its_parts() {
        let repr = Transform2d::from_parts([3.0, -4.0], 0.5, [2.0, 0.25]).to_repr();
        let global = GlobalTransform2d::from_repr(repr);
        assert_eq!(global.translation(), [3.0, -4.0]);
        assert_eq!(global.rotation(), 0.5);
        assert_eq!(global.scale(), [2.0, 0.25]);
        assert_eq!(
            global.to_transform(),
            Transform2d::from_parts([3.0, -4.0], 0.5, [2.0, 0.25])
        );
    }

    #[test]
    fn defaults_are_the_identity() {
        assert_eq!(Transform2d::default(), Transform2d::IDENTITY);
        assert_eq!(GlobalTransform2d::default(), GlobalTransform2d::IDENTITY);
    }
}
