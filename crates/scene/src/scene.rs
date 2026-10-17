//! Scenes: a world, a schedule, and the update order that drives them.

use core::fmt;
use core::time::Duration;

use mikan_ecs::{Entity, Schedule, World};
use mikan_scene_sys::Transform2dRepr;

use crate::hierarchy::HierarchyError;
use crate::transform::{GlobalTransform2d, Transform2d};
use crate::{Hierarchy, flush_despawns, request_despawn};

/// One game scene: entities, systems, and the fixed update order.
///
/// A scene owns exactly one [`World`] and one [`Schedule`] and nothing else.
/// The hierarchy is stored *in* the world as a resource
/// (inserted by [`Scene::new`]), so systems can query parent links during a
/// step; [`Scene::update`] then flushes deferred despawns and recomputes
/// world transforms (see the crate docs for the exact order and why).
///
/// ```
/// use std::time::Duration;
///
/// use mikan_scene::{Scene, Transform2d};
///
/// let mut scene = Scene::new("demo");
/// let entity = scene.spawn();
/// scene
///     .world_mut()
///     .insert(entity, Transform2d::from_translation([1.0, 2.0]))?;
/// scene.update(Duration::ZERO);
/// assert_eq!(scene.world().entity_count(), 1);
/// # Ok::<(), mikan_ecs::DeadEntity>(())
/// ```
pub struct Scene {
    name: String,
    world: World,
    schedule: Schedule,
}

impl Scene {
    /// Creates an empty scene with the given diagnostic name.
    ///
    /// The scene starts with one resource: an empty [`Hierarchy`]. Everything
    /// else — entities, systems, transform propagation — is built through the
    /// accessors below. Construction cannot fail, so scenes are always
    /// complete values: there is no half-initialized state a manager could
    /// observe.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let mut world = World::new();
        world.insert_resource(Hierarchy::new());
        Self {
            name: name.into(),
            world,
            schedule: Schedule::new(),
        }
    }

    /// The scene's diagnostic name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The scene's world.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The scene's world, for spawning and component edits outside systems.
    ///
    /// Entities spawned through this accessor are registered with the
    /// hierarchy by the next [`propagate_transforms`], not immediately; use
    /// [`Scene::spawn`] when the entity must be a hierarchy node right away
    /// (for example, to be a parent before the next update).
    #[must_use]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The scene's schedule, for adding systems.
    ///
    /// Systems run once per [`Scene::update`] call, in registration order,
    /// with the `dt` that call received. Adding systems after the first
    /// update is legal but unusual: the next update runs the new list.
    #[must_use]
    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }

    /// The names of the systems, in execution order.
    pub fn systems(&self) -> impl ExactSizeIterator<Item = &str> {
        self.schedule.names()
    }

    /// Number of live entities.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.world.entity_count()
    }

    /// Spawns an entity and registers it as a detached hierarchy root.
    ///
    /// The entity starts with no components; insert a [`Transform2d`] to give
    /// it a place, and attach it with [`Scene::set_parent`] to give it an
    /// ancestry. Because the node exists immediately, a fresh entity can be
    /// made a parent before the next update.
    pub fn spawn(&mut self) -> Entity {
        let entity = self.world.spawn();
        self.hierarchy_mut()
            .add(entity)
            .expect("a freshly spawned entity is never already in the hierarchy");
        entity
    }

    /// Destroys an entity immediately: removes its hierarchy node (re-rooting
    /// its children) and despawns it from the world.
    ///
    /// Returns `false` (and changes nothing) if the entity is not alive.
    /// Prefer [`Scene::despawn_later`] inside systems, which cannot hold the
    /// world mutably.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.world.is_alive(entity) {
            return false;
        }
        let _ = self.hierarchy_mut().remove(entity);
        self.world.despawn(entity)
    }

    /// Requests `entity`'s destruction at the next flush point.
    ///
    /// See [`request_despawn`]; [`Scene::update`] applies the queue after the
    /// schedule and before propagation.
    pub fn despawn_later(&mut self, entity: Entity) {
        request_despawn(&mut self.world, entity);
    }

    /// Attaches `child` under `parent`, detaching it from any previous
    /// parent. Local transforms are never rewritten by topology changes.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if either entity is not alive,
    /// [`HierarchyError::Cycle`] if the link would close a cycle.
    pub fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<(), HierarchyError> {
        if !self.world.is_alive(child) {
            return Err(HierarchyError::UnknownEntity(child));
        }
        if !self.world.is_alive(parent) {
            return Err(HierarchyError::UnknownEntity(parent));
        }
        self.hierarchy_mut().set_parent(child, parent)
    }

    /// Detaches `entity` from its parent; it becomes a root.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not alive.
    pub fn clear_parent(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        if !self.world.is_alive(entity) {
            return Err(HierarchyError::UnknownEntity(entity));
        }
        self.hierarchy_mut().clear_parent(entity)
    }

    /// The parent of `entity`, or `None` when it is a root.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not alive.
    pub fn parent(&self, entity: Entity) -> Result<Option<Entity>, HierarchyError> {
        if !self.world.is_alive(entity) {
            return Err(HierarchyError::UnknownEntity(entity));
        }
        self.hierarchy().parent(entity)
    }

    /// The scene's hierarchy, for read-only queries.
    #[must_use]
    pub fn hierarchy(&self) -> &Hierarchy {
        self.world
            .get_resource::<Hierarchy>()
            .expect("a scene always owns its hierarchy")
    }

    /// The scene's hierarchy, for direct topology edits.
    ///
    /// Edits through this accessor bypass the aliveness checks of
    /// [`Scene::set_parent`]; the next [`propagate_transforms`] prunes any
    /// node whose entity is dead.
    #[must_use]
    pub fn hierarchy_mut(&mut self) -> &mut Hierarchy {
        self.world
            .get_resource_mut::<Hierarchy>()
            .expect("a scene always owns its hierarchy")
    }

    /// Advances the scene by one fixed step of `dt`:
    ///
    /// 1. run every system once, in registration order;
    /// 2. flush deferred despawns ([`request_despawn`]);
    /// 3. recompute every world transform ([`propagate_transforms`]).
    ///
    /// This is the only place the engine drives a scene's systems; the
    /// platform layer decides how many times per frame it is called, exactly
    /// as `mikan-ecs` requires.
    pub fn update(&mut self, dt: Duration) {
        self.schedule.run(&mut self.world, dt);
        flush_despawns(&mut self.world);
        propagate_transforms(&mut self.world);
    }

    /// Recomputes world transforms without running the schedule — for the
    /// rare caller that edited local transforms outside [`Scene::update`].
    pub fn propagate(&mut self) {
        propagate_transforms(&mut self.world);
    }
}

impl fmt::Debug for Scene {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Scene")
            .field("name", &self.name)
            .field("entities", &self.world.entity_count())
            .field("systems", &self.schedule)
            .finish()
    }
}

/// Recomputes every hierarchy entity's [`GlobalTransform2d`] from its
/// [`Transform2d`] and its ancestry, in three phases:
///
/// 1. **Prune**: dead entities leave the hierarchy; their children become
///    roots (with their local transforms unchanged, so their world
///    transforms are recomputed from the roots down).
/// 2. **Reconcile**: live entities that hold a `Transform2d` but are not yet
///    nodes — spawned through [`World::spawn`](mikan_ecs::World::spawn)
///    directly — are added as roots.
/// 3. **Compose and write back**: locals are gathered in node order, the
///    native kernel composes world transforms parents-first in one batch
///    call, and one `GlobalTransform2d` is written per node.
///
/// With no [`Hierarchy`] resource in `world` the function returns without
/// touching anything: a bare ECS world pays nothing for a system it does not
/// use.
///
/// An entity whose `Transform2d` was removed while it stayed alive
/// contributes the identity transform; it keeps a derived
/// `GlobalTransform2d` (the cache reports where the entity *is*, and with no
/// local transform that is the parent's frame).
pub fn propagate_transforms(world: &mut World) {
    let Some(mut hierarchy) = world.remove_resource::<Hierarchy>() else {
        return;
    };

    // Phase 1: prune dead nodes. Collected first because removal borrows the
    // hierarchy mutably while iteration walks it.
    for entity in hierarchy.entities() {
        if !world.is_alive(entity) {
            let _ = hierarchy.remove(entity);
        }
    }

    // Phase 2: reconcile live entities that have a local transform.
    let newcomers: Vec<Entity> = world
        .iter::<Transform2d>()
        .map(|(entity, _)| entity)
        .filter(|entity| !hierarchy.contains(*entity))
        .collect();
    for entity in newcomers {
        // A live entity that is not a node cannot already be present, so the
        // only possible error is an id collision that cannot happen.
        let _ = hierarchy.add(entity);
    }

    if !hierarchy.is_empty() {
        let nodes = hierarchy.entities();
        let mut locals = Vec::with_capacity(nodes.len());
        for &entity in &nodes {
            let local = world
                .get::<Transform2d>(entity)
                .copied()
                .unwrap_or(Transform2d::IDENTITY);
            locals.push(local.to_repr());
        }
        let mut world_transforms = vec![Transform2dRepr::identity(); nodes.len()];
        hierarchy
            .propagate(&locals, &mut world_transforms)
            .expect("locals and output are sized to the hierarchy");
        for (entity, repr) in nodes.iter().zip(&world_transforms) {
            world
                .insert(*entity, GlobalTransform2d::from_repr(*repr))
                .expect("hierarchy nodes are live entities");
        }
    }

    world.insert_resource(hierarchy);
}
