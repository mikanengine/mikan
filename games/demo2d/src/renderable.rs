//! Drawing ECS entities: a [`SpriteRenderer`] component plus the world
//! transform the scene propagated for the entity.
//!
//! This is the bridge between the entity/scene layer (`mikan-ecs` and
//! `mikan-scene`) and the renderer (`mikan-render`). An entity is drawn exactly
//! when it carries both:
//!
//! - a [`SpriteRenderer`], which says *what* to draw — the size and one of the
//!   [`Art`] sources; and
//! - a `GlobalTransform2d`, which says *where* — recomputed from the entity's
//!   `Transform2d` and its ancestry every fixed step ([`Scene::update`]).
//!
//! Neither half knows about the other: a system moves a `Transform2d`, the
//! scene propagates it, and this module turns the pair into a [`Sprite`] in one
//! pass, in `SpriteRenderer` insertion order. That order is the painter order,
//! so the demo spawns its blocks before the player, exactly as the pre-ECS
//! drawing did.
//!
//! [`Scene::update`]: mikan::Scene::update

use mikan::{Color, Entity, GlobalTransform2d, SamplerMode, Sprite, UvRect, World};

use crate::assets::Assets;

/// What a [`SpriteRenderer`] draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Art {
    /// A flat rectangle of this color, sampling no texture.
    Solid(Color),
    /// A frame of the loaded atlas, selected by its UV rectangle.
    Atlas(UvRect),
    /// The standalone star texture, with transparent corners.
    Star,
}

/// Makes an entity drawable: its size and what it samples.
///
/// The position, rotation, and (eventually) scale come from the entity's
/// propagated `GlobalTransform2d`, never from the component, so moving an
/// entity is a single edit to its `Transform2d`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteRenderer {
    /// Full width and height in world pixels.
    pub size: [f32; 2],
    /// What to draw at that size.
    pub art: Art,
}

impl SpriteRenderer {
    /// A flat colored rectangle, like [`Sprite::solid`].
    #[must_use]
    pub const fn solid(size: [f32; 2], color: Color) -> Self {
        Self {
            size,
            art: Art::Solid(color),
        }
    }

    /// A frame of the atlas.
    #[must_use]
    pub const fn atlas(size: [f32; 2], uv: UvRect) -> Self {
        Self {
            size,
            art: Art::Atlas(uv),
        }
    }

    /// The standalone star texture.
    #[must_use]
    pub const fn star(size: [f32; 2]) -> Self {
        Self {
            size,
            art: Art::Star,
        }
    }
}

/// Appends one [`Sprite`] per drawable entity, in `SpriteRenderer` insertion
/// order.
///
/// `override_position` replaces the world position of one entity — the classic
/// player, whose render pose is interpolated between the last two fixed states
/// so motion stays smooth at frame rates that are not a multiple of the fixed
/// update rate. Passing `None` draws every entity at its propagated position.
///
/// The scene-wide `sampler` is applied to every textured sprite. The demo has
/// no per-entity sampler overrides, so there is nothing to pin here.
pub fn collect<'a>(
    world: &World,
    assets: &'a Assets,
    sampler: SamplerMode,
    override_position: Option<(Entity, [f32; 2])>,
    out: &mut Vec<Sprite<'a>>,
) {
    for (entity, renderer, global) in world.iter2::<SpriteRenderer, GlobalTransform2d>() {
        let center = match override_position {
            Some((target, position)) if target == entity => position,
            _ => global.translation(),
        };
        let sprite = match renderer.art {
            Art::Solid(color) => Sprite::solid(center, renderer.size, color),
            Art::Atlas(uv) => Sprite::new(center, renderer.size, &assets.atlas)
                .with_uv(uv)
                .with_sampler(sampler),
            Art::Star => Sprite::new(center, renderer.size, &assets.star).with_sampler(sampler),
        };
        out.push(sprite.with_rotation(global.rotation()));
    }
}
