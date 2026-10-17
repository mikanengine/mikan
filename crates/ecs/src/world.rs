//! The world: entities, typed component storages, and resources.
//!
//! Component storage is deliberately simple and explicit:
//!
//! - Every component type gets its own storage, created on first insert and
//!   keyed by `TypeId`. Inserting `Position` and `Velocity` for an entity
//!   stores two independent values.
//! - A storage is a sparse set: a per-slot index into a dense, insertion
//!   ordered list of `(Entity, T)` entries. Iteration always visits entries in
//!   the order they were first inserted, and removing an entry keeps the
//!   relative order of the rest.
//! - Despawning an entity removes all of its components from every storage.
//!   A stale handle can therefore never observe another entity's components:
//!   lookups compare the full handle, generation included.
//!
//! The cost of this design is paid on removal — an order-preserving removal
//! shifts the entries after it — and it is the right trade for a "lite" ECS:
//! deterministic iteration order is worth more to gameplay code than removal
//! throughput. See `docs/adr/0009-ecs-lite.md`.

use core::any::{Any, TypeId};
use core::fmt;
use core::iter::FusedIterator;
use core::slice;
use std::collections::HashMap;

use crate::entity::{Entities, Entity};
use crate::error::DeadEntity;

/// The storage of one component type: a sparse set over entity slots.
///
/// Invariants (kept by construction, checked by debug assertions):
///
/// - `sparse[index]` is `Some(position)` exactly when `entries[position].0`
///   has that index, and then the positions agree.
/// - `entries` contains no two entries for the same entity.
#[derive(Debug)]
struct SparseSet<T> {
    /// Per entity slot: position in `entries`, or `None`.
    sparse: Vec<Option<u32>>,
    /// `(owner, value)` pairs in insertion order.
    entries: Vec<(Entity, T)>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entries: Vec::new(),
        }
    }
}

impl<T> SparseSet<T> {
    /// Position in `entries` of `entity`'s component, if any.
    fn slot(&self, entity: Entity) -> Option<usize> {
        let position = (*self.sparse.get(entity.index() as usize)?)? as usize;
        (self.entries[position].0 == entity).then_some(position)
    }

    fn get(&self, entity: Entity) -> Option<&T> {
        let position = self.slot(entity)?;
        Some(&self.entries[position].1)
    }

    fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let position = self.slot(entity)?;
        Some(&mut self.entries[position].1)
    }

    /// Inserts or replaces the entity's component.
    fn insert(&mut self, entity: Entity, value: T) {
        let index = entity.index() as usize;
        if self.sparse.len() <= index {
            self.sparse.resize(index + 1, None);
        }
        match self.sparse[index] {
            Some(position) => {
                let entry = &mut self.entries[position as usize];
                debug_assert_eq!(
                    entry.0, entity,
                    "internal: a slot's component must belong to the live entity"
                );
                entry.1 = value;
            }
            None => {
                self.sparse[index] = Some(self.entries.len() as u32);
                self.entries.push((entity, value));
            }
        }
    }

    /// Removes and returns the entity's component, keeping the order of the
    /// remaining entries.
    fn remove(&mut self, entity: Entity) -> Option<T> {
        let position = self.slot(entity)?;
        let (_, value) = self.entries.remove(position);
        self.sparse[entity.index() as usize] = None;
        // Every entry after the hole moved one position left.
        for (owner, _) in &self.entries[position..] {
            if let Some(slot) = self.sparse[owner.index() as usize].as_mut() {
                *slot -= 1;
            }
        }
        Some(value)
    }

    fn iter(&self) -> slice::Iter<'_, (Entity, T)> {
        self.entries.iter()
    }

    fn iter_mut(&mut self) -> slice::IterMut<'_, (Entity, T)> {
        self.entries.iter_mut()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// A component storage as the world sees it: type-erased so all storages can
/// live in one map, but with the one operation the world performs generically.
trait ErasedStorage {
    /// Removes the (single) component `entity` may have in this storage.
    fn remove_entity(&mut self, entity: Entity);

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> ErasedStorage for SparseSet<T> {
    fn remove_entity(&mut self, entity: Entity) {
        self.remove(entity);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Entities, their components, and shared resources.
///
/// A world is a plain value: it owns all storage, has no global state, and can
/// be dropped, moved, and rebuilt. It is single-threaded — iteration hands out
/// references tied to `&self` — but nothing stops a program from keeping
/// several worlds side by side.
///
/// # Example
///
/// ```
/// use mikan_ecs::World;
///
/// struct Position([f32; 2]);
///
/// let mut world = World::new();
/// let marker = world.spawn();
/// world.insert(marker, Position([10.0, 20.0]))?;
///
/// for (entity, position) in world.iter::<Position>() {
///     assert_eq!(entity, marker);
///     assert_eq!(position.0, [10.0, 20.0]);
/// }
///
/// let stale = world.spawn();
/// world.despawn(stale);
/// assert!(!world.despawn(stale), "the second despawn is a no-op");
/// # Ok::<(), mikan_ecs::DeadEntity>(())
/// ```
#[derive(Default)]
pub struct World {
    entities: Entities,
    storages: HashMap<TypeId, Box<dyn ErasedStorage>>,
    resources: HashMap<TypeId, Box<dyn Any>>,
}

impl World {
    /// Creates an empty world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns an entity and returns its handle.
    ///
    /// The entity starts with no components. Slot reuse is LIFO; a reused slot
    /// always comes back with a new generation, so handles to the previous
    /// occupant stay invalid.
    pub fn spawn(&mut self) -> Entity {
        self.entities.spawn()
    }

    /// Despawns an entity, removing all of its components.
    ///
    /// Returns whether the entity was alive; despawning a stale handle is a
    /// no-op that returns `false`.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.despawn(entity) {
            return false;
        }
        for storage in self.storages.values_mut() {
            storage.remove_entity(entity);
        }
        true
    }

    /// Whether `entity` is alive in this world.
    #[must_use]
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Number of live entities, including those with no components.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Number of live `T` components stored in this world.
    #[must_use]
    pub fn component_count<T: 'static>(&self) -> usize {
        self.storage::<T>().map_or(0, SparseSet::len)
    }

    /// Attaches `component` to `entity`.
    ///
    /// An existing component of the same type is replaced. Returns
    /// [`DeadEntity`] without changing anything if the entity is not alive.
    ///
    /// # Errors
    ///
    /// Fails when `entity` is not alive in this world.
    pub fn insert<T: 'static>(&mut self, entity: Entity, component: T) -> Result<(), DeadEntity> {
        if !self.entities.is_alive(entity) {
            return Err(DeadEntity { entity });
        }
        self.storage_mut::<T>().insert(entity, component);
        Ok(())
    }

    /// Removes and returns `entity`'s `T` component, if it has one.
    pub fn remove<T: 'static>(&mut self, entity: Entity) -> Option<T> {
        self.storage_mut_opt::<T>()?.remove(entity)
    }

    /// Returns a reference to `entity`'s `T` component.
    #[must_use]
    pub fn get<T: 'static>(&self, entity: Entity) -> Option<&T> {
        self.storage::<T>()?.get(entity)
    }

    /// Returns a mutable reference to `entity`'s `T` component.
    #[must_use]
    pub fn get_mut<T: 'static>(&mut self, entity: Entity) -> Option<&mut T> {
        self.storage_mut_opt::<T>()?.get_mut(entity)
    }

    /// Whether `entity` has a `T` component.
    #[must_use]
    pub fn has<T: 'static>(&self, entity: Entity) -> bool {
        self.get::<T>(entity).is_some()
    }

    /// Iterates over every `(Entity, &T)` in insertion order.
    ///
    /// An entity that never had a `T` component is never visited; a dead
    /// entity is never visited, because despawn removes every component.
    /// Iteration over a missing component type yields nothing.
    #[must_use]
    pub fn iter<T: 'static>(&self) -> Iter<'_, T> {
        match self.storage::<T>() {
            Some(storage) => Iter {
                inner: storage.iter(),
            },
            None => Iter::empty(),
        }
    }

    /// Iterates over every `(Entity, &mut T)` in insertion order.
    #[must_use]
    pub fn iter_mut<T: 'static>(&mut self) -> IterMut<'_, T> {
        match self.storage_mut_opt::<T>() {
            Some(storage) => IterMut {
                inner: storage.iter_mut(),
            },
            None => IterMut::empty(),
        }
    }

    /// Iterates over every entity that has both an `A` and a `B` component,
    /// in `A`'s insertion order.
    ///
    /// Both references are shared, so `A` and `B` may even be the same type.
    #[must_use]
    pub fn iter2<A: 'static, B: 'static>(&self) -> Iter2<'_, A, B> {
        Iter2 {
            a: self.iter::<A>().inner,
            b: self.storage::<B>(),
        }
    }

    /// Iterates over every entity that has both components, yielding `A`
    /// mutably and `B` shared, in `A`'s insertion order.
    ///
    /// The two references point into different storages, which is what makes
    /// "read `Velocity`, write `Position`" a single safe loop.
    ///
    /// # Panics
    ///
    /// Panics if `A` and `B` are the same type: two references to one
    /// component cannot be handed out safely. Use [`World::iter_mut`] for a
    /// single mutable component type.
    #[must_use]
    pub fn iter2_mut<A: 'static, B: 'static>(&mut self) -> Iter2Mut<'_, A, B> {
        assert!(
            TypeId::of::<A>() != TypeId::of::<B>(),
            "iter2_mut requires two different component types"
        );
        let [a, b] = self
            .storages
            .get_disjoint_mut([&TypeId::of::<A>(), &TypeId::of::<B>()]);
        let (Some(a), Some(b)) = (a, b) else {
            return Iter2Mut::empty();
        };
        let (Some(a), Some(b)) = (
            a.as_any_mut().downcast_mut::<SparseSet<A>>(),
            b.as_any_mut().downcast_mut::<SparseSet<B>>(),
        ) else {
            return Iter2Mut::empty();
        };
        let b: &SparseSet<B> = b;
        Iter2Mut {
            a: a.iter_mut(),
            b: Some(b),
        }
    }

    /// Stores `resource`, returning the previous value of the same type.
    ///
    /// Resources are world-wide singletons — one value per type — for data
    /// that systems share but no single entity owns: input for this step,
    /// view bounds, configuration, event queues. They are independent of
    /// component storage: the same type can be both a component and a
    /// resource without interfering.
    pub fn insert_resource<T: 'static>(&mut self, resource: T) -> Option<T> {
        self.resources
            .insert(TypeId::of::<T>(), Box::new(resource))
            .and_then(|previous| previous.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    /// Returns a reference to the `T` resource, if one is stored.
    #[must_use]
    pub fn get_resource<T: 'static>(&self) -> Option<&T> {
        self.resources.get(&TypeId::of::<T>())?.downcast_ref::<T>()
    }

    /// Returns a mutable reference to the `T` resource, if one is stored.
    #[must_use]
    pub fn get_resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())?
            .downcast_mut::<T>()
    }

    /// Removes and returns the `T` resource, if one is stored.
    pub fn remove_resource<T: 'static>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeId::of::<T>())?
            .downcast::<T>()
            .ok()
            .map(|boxed| *boxed)
    }

    /// The storage of `T`, if any of it was ever inserted.
    fn storage<T: 'static>(&self) -> Option<&SparseSet<T>> {
        self.storages
            .get(&TypeId::of::<T>())?
            .as_any()
            .downcast_ref::<SparseSet<T>>()
    }

    /// The storage of `T` for mutation, if it exists.
    fn storage_mut_opt<T: 'static>(&mut self) -> Option<&mut SparseSet<T>> {
        self.storages
            .get_mut(&TypeId::of::<T>())?
            .as_any_mut()
            .downcast_mut::<SparseSet<T>>()
    }

    /// The storage of `T` for mutation, created on first use.
    fn storage_mut<T: 'static>(&mut self) -> &mut SparseSet<T> {
        self.storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(SparseSet::<T>::default()))
            .as_any_mut()
            .downcast_mut::<SparseSet<T>>()
            .expect("internal: a storage is keyed by the TypeId of its own component")
    }
}

impl fmt::Debug for World {
    /// Storage is unnameable through a trait object, so the summary reports
    /// what can be observed: counts, not contents.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("World")
            .field("entities", &self.entities.len())
            .field("component_storages", &self.storages.len())
            .field("resources", &self.resources.len())
            .finish()
    }
}

/// Iterator over `(Entity, &T)` pairs; see [`World::iter`].
pub struct Iter<'a, T> {
    inner: slice::Iter<'a, (Entity, T)>,
}

impl<'a, T> Iter<'a, T> {
    fn empty() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (Entity, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let (entity, value) = self.inner.next()?;
        Some((*entity, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

/// Iterator over `(Entity, &mut T)` pairs; see [`World::iter_mut`].
pub struct IterMut<'a, T> {
    inner: slice::IterMut<'a, (Entity, T)>,
}

impl<'a, T> IterMut<'a, T> {
    fn empty() -> Self {
        Self {
            inner: Default::default(),
        }
    }
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (Entity, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let (entity, value) = self.inner.next()?;
        Some((*entity, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> FusedIterator for IterMut<'_, T> {}

/// Iterator over `(Entity, &A, &B)` triples; see [`World::iter2`].
pub struct Iter2<'a, A, B> {
    a: slice::Iter<'a, (Entity, A)>,
    b: Option<&'a SparseSet<B>>,
}

impl<'a, A, B> Iterator for Iter2<'a, A, B> {
    type Item = (Entity, &'a A, &'a B);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.b?;
        loop {
            let (entity, a) = self.a.next()?;
            if let Some(b) = b.get(*entity) {
                return Some((*entity, a, b));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.a.len()))
    }
}

impl<A, B> FusedIterator for Iter2<'_, A, B> {}

/// Iterator over `(Entity, &mut A, &B)` triples; see [`World::iter2_mut`].
pub struct Iter2Mut<'a, A, B> {
    a: slice::IterMut<'a, (Entity, A)>,
    b: Option<&'a SparseSet<B>>,
}

impl<'a, A, B> Iter2Mut<'a, A, B> {
    fn empty() -> Self {
        Self {
            a: Default::default(),
            b: None,
        }
    }
}

impl<'a, A, B> Iterator for Iter2Mut<'a, A, B> {
    type Item = (Entity, &'a mut A, &'a B);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.b?;
        loop {
            let (entity, a) = self.a.next()?;
            if let Some(b) = b.get(*entity) {
                return Some((*entity, a, b));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.a.len()))
    }
}

impl<A, B> FusedIterator for Iter2Mut<'_, A, B> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Position([f32; 2]);

    #[derive(Debug, PartialEq)]
    struct Velocity([f32; 2]);

    #[test]
    fn spawn_makes_an_alive_entity() {
        let mut world = World::new();
        let entity = world.spawn();
        assert!(world.is_alive(entity));
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn insert_then_get_returns_the_component() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        assert_eq!(world.get::<Position>(entity), Some(&Position([1.0, 2.0])));
        assert!(world.has::<Position>(entity));
    }

    #[test]
    fn get_mut_edits_the_component() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        world.get_mut::<Position>(entity).unwrap().0 = [3.0, 4.0];
        assert_eq!(world.get::<Position>(entity), Some(&Position([3.0, 4.0])));
    }

    #[test]
    fn component_types_do_not_interfere() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        world.insert(entity, Velocity([3.0, 4.0])).unwrap();
        assert_eq!(world.get::<Position>(entity), Some(&Position([1.0, 2.0])));
        assert_eq!(world.get::<Velocity>(entity), Some(&Velocity([3.0, 4.0])));
        assert!(!world.has::<f32>(entity), "unrelated types stay absent");
    }

    #[test]
    fn insert_replaces_the_existing_component() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        world.insert(entity, Position([9.0, 9.0])).unwrap();
        assert_eq!(world.get::<Position>(entity), Some(&Position([9.0, 9.0])));
        assert_eq!(world.component_count::<Position>(), 1);
    }

    #[test]
    fn remove_returns_the_component_and_clears_the_slot() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        assert_eq!(world.remove::<Position>(entity), Some(Position([1.0, 2.0])));
        assert_eq!(world.remove::<Position>(entity), None);
        assert!(!world.has::<Position>(entity));
    }

    #[test]
    fn insert_on_a_dead_entity_is_an_error_carrying_the_entity() {
        let mut world = World::new();
        let entity = world.spawn();
        world.despawn(entity);
        let error = world.insert(entity, Position([1.0, 2.0])).unwrap_err();
        assert_eq!(error.entity, entity);
        assert!(
            error.to_string().contains(&entity.to_string()),
            "the message names the entity: {error}"
        );
        assert_eq!(world.component_count::<Position>(), 0);
    }

    #[test]
    fn a_stale_handle_after_slot_reuse_is_rejected() {
        let mut world = World::new();
        let first = world.spawn();
        world.insert(first, Position([1.0, 2.0])).unwrap();
        world.despawn(first);
        let second = world.spawn();
        assert_eq!(second.index(), first.index(), "the slot is reused");
        assert!(world.insert(first, Position([3.0, 4.0])).is_err());
        assert_eq!(world.get::<Position>(first), None);
        world.insert(second, Position([5.0, 6.0])).unwrap();
        assert_eq!(world.get::<Position>(first), None, "handles stay distinct");
        assert_eq!(world.get::<Position>(second), Some(&Position([5.0, 6.0])));
    }

    #[test]
    fn despawn_removes_every_component() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        world.insert(entity, Velocity([3.0, 4.0])).unwrap();
        assert!(world.despawn(entity));
        assert!(!world.is_alive(entity));
        assert_eq!(world.component_count::<Position>(), 0);
        assert_eq!(world.component_count::<Velocity>(), 0);
        assert_eq!(world.iter::<Position>().count(), 0);
        assert_eq!(world.iter::<Velocity>().count(), 0);
    }

    #[test]
    fn despawn_of_a_dead_entity_is_a_no_op() {
        let mut world = World::new();
        let entity = world.spawn();
        assert!(world.despawn(entity));
        assert!(!world.despawn(entity));
        assert_eq!(world.entity_count(), 0);
    }

    #[test]
    fn entities_are_independent() {
        let mut world = World::new();
        let a = world.spawn();
        let b = world.spawn();
        world.insert(a, Position([1.0, 1.0])).unwrap();
        world.insert(b, Position([2.0, 2.0])).unwrap();
        world.get_mut::<Position>(a).unwrap().0 = [7.0, 7.0];
        assert_eq!(world.get::<Position>(a), Some(&Position([7.0, 7.0])));
        assert_eq!(world.get::<Position>(b), Some(&Position([2.0, 2.0])));
    }

    #[test]
    fn component_count_tracks_live_components() {
        let mut world = World::new();
        let a = world.spawn();
        let b = world.spawn();
        world.insert(a, Position([1.0, 1.0])).unwrap();
        world.insert(b, Position([2.0, 2.0])).unwrap();
        assert_eq!(world.component_count::<Position>(), 2);
        world.remove::<Position>(a);
        assert_eq!(world.component_count::<Position>(), 1);
        world.despawn(b);
        assert_eq!(world.component_count::<Position>(), 0);
    }

    #[test]
    fn iteration_visits_entities_in_insertion_order() {
        let mut world = World::new();
        let entities: Vec<Entity> = (0..3)
            .map(|_| {
                let entity = world.spawn();
                world.insert(entity, Position([0.0, 0.0])).unwrap();
                entity
            })
            .collect();
        let visited: Vec<Entity> = world.iter::<Position>().map(|(e, _)| e).collect();
        assert_eq!(visited, entities);
    }

    #[test]
    fn iteration_order_survives_a_middle_removal() {
        let mut world = World::new();
        let a = world.spawn();
        let b = world.spawn();
        let c = world.spawn();
        for entity in [a, b, c] {
            world.insert(entity, Position([0.0, 0.0])).unwrap();
        }
        world.remove::<Position>(b);
        let visited: Vec<Entity> = world.iter::<Position>().map(|(e, _)| e).collect();
        assert_eq!(visited, [a, c], "c keeps its position after b leaves");
    }

    #[test]
    fn iter_mut_exposes_mutable_access() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        for (_, position) in world.iter_mut::<Position>() {
            position.0[0] += 10.0;
        }
        assert_eq!(world.get::<Position>(entity), Some(&Position([11.0, 2.0])));
    }

    #[test]
    fn iter_on_a_missing_component_type_is_empty() {
        let mut world = World::new();
        world.spawn();
        assert_eq!(world.iter::<Position>().count(), 0);
        assert_eq!(world.iter_mut::<Position>().count(), 0);
        assert_eq!(world.iter::<Position>().size_hint(), (0, Some(0)));
    }

    #[test]
    fn iter2_yields_only_entities_with_both_components() {
        let mut world = World::new();
        let both = world.spawn();
        let only_position = world.spawn();
        world.insert(both, Position([1.0, 1.0])).unwrap();
        world.insert(both, Velocity([2.0, 2.0])).unwrap();
        world.insert(only_position, Position([3.0, 3.0])).unwrap();
        let visited: Vec<(Entity, [f32; 2])> = world
            .iter2::<Position, Velocity>()
            .map(|(entity, position, _)| (entity, position.0))
            .collect();
        assert_eq!(visited, [(both, [1.0, 1.0])]);
    }

    #[test]
    fn iter2_of_a_missing_type_is_empty() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 1.0])).unwrap();
        assert_eq!(world.iter2::<Position, Velocity>().count(), 0);
        assert_eq!(world.iter2::<Velocity, Position>().count(), 0);
    }

    #[test]
    fn iter2_mut_writes_one_component_and_reads_the_other() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([0.0, 0.0])).unwrap();
        world.insert(entity, Velocity([3.0, -1.0])).unwrap();
        let other = world.spawn();
        world.insert(other, Position([5.0, 5.0])).unwrap();
        for (_, position, velocity) in world.iter2_mut::<Position, Velocity>() {
            position.0[0] += velocity.0[0];
            position.0[1] += velocity.0[1];
        }
        assert_eq!(world.get::<Position>(entity), Some(&Position([3.0, -1.0])));
        assert_eq!(world.get::<Position>(other), Some(&Position([5.0, 5.0])));
    }

    #[test]
    #[should_panic(expected = "two different component types")]
    fn iter2_mut_with_the_same_type_panics() {
        let mut world = World::new();
        let _ = world.iter2_mut::<Position, Position>();
    }

    #[test]
    fn resources_round_trip() {
        let mut world = World::new();
        assert_eq!(world.get_resource::<u32>(), None);
        assert_eq!(world.insert_resource(7_u32), None);
        assert_eq!(world.get_resource::<u32>(), Some(&7));
        *world.get_resource_mut::<u32>().unwrap() = 8;
        assert_eq!(world.get_resource::<u32>(), Some(&8));
        assert_eq!(world.remove_resource::<u32>(), Some(8));
        assert_eq!(world.get_resource::<u32>(), None);
    }

    #[test]
    fn inserting_a_resource_returns_the_previous_value() {
        let mut world = World::new();
        world.insert_resource(String::from("first"));
        let previous = world.insert_resource(String::from("second"));
        assert_eq!(previous.as_deref(), Some("first"));
        assert_eq!(
            world.get_resource::<String>().map(String::as_str),
            Some("second")
        );
    }

    #[test]
    fn a_type_can_be_both_a_component_and_a_resource() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, 5_u32).unwrap();
        world.insert_resource(9_u32);
        assert_eq!(world.get::<u32>(entity), Some(&5));
        assert_eq!(world.get_resource::<u32>(), Some(&9));
        world.despawn(entity);
        assert_eq!(
            world.get_resource::<u32>(),
            Some(&9),
            "despawn keeps resources"
        );
    }

    #[test]
    fn debug_reports_counts_not_contents() {
        let mut world = World::new();
        let entity = world.spawn();
        world.insert(entity, Position([1.0, 2.0])).unwrap();
        world.insert_resource(1_u8);
        let debug = format!("{world:?}");
        assert!(debug.contains("entities: 1"), "{debug}");
        assert!(debug.contains("component_storages: 1"), "{debug}");
        assert!(debug.contains("resources: 1"), "{debug}");
    }
}
