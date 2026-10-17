//! Deferred destruction: the despawn queue.
//!
//! Systems iterate the world through borrows, so a system cannot call
//! `World::despawn` in the middle of its own loop. The scene system's answer
//! is a request list: a system calls [`request_despawn`], which only records
//! the entity, and [`Scene::update`](crate::Scene::update) applies every
//! request — after the schedule, before propagation — through
//! [`flush_despawns`].
//!
//! Flushing is the full destruction path: the entity leaves the hierarchy
//! (its children become roots, per [`Hierarchy::remove`](crate::Hierarchy::remove))
//! and then the world despawns it, removing every component. A renderer that
//! collects components through the world therefore cannot see a destroyed
//! entity, whether it was despawned immediately or through the queue.

use mikan_ecs::{Entity, World};

use crate::Hierarchy;

/// Entities requested for destruction, applied by
/// [`flush_despawns`](crate::flush_despawns).
///
/// Stored as a world resource, created by the first
/// [`request_despawn`](crate::request_despawn) call and removed by the flush
/// that applies it. A duplicate request is harmful only to the request list's
/// length: despawns are idempotent, so flushing `[a, a]` despawns `a` once.
#[derive(Debug, Default)]
pub struct DespawnQueue {
    requested: Vec<Entity>,
}

impl DespawnQueue {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `entity` for destruction.
    pub fn request(&mut self, entity: Entity) {
        self.requested.push(entity);
    }

    /// The entities requested so far, in request order.
    pub fn requested(&self) -> impl ExactSizeIterator<Item = Entity> + '_ {
        self.requested.iter().copied()
    }

    /// Number of pending requests (including duplicates).
    #[must_use]
    pub fn len(&self) -> usize {
        self.requested.len()
    }

    /// Whether there are no pending requests.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.requested.is_empty()
    }

    /// Discards every pending request without applying it.
    pub fn clear(&mut self) {
        self.requested.clear();
    }
}

/// Requests `entity`'s destruction at the next flush.
///
/// Safe to call from inside a system: it touches only the queue resource.
/// Requesting an entity that is already dead (or was already despawned) is
/// allowed; the flush skips it, and the count it returns is the number of
/// entities actually destroyed.
pub fn request_despawn(world: &mut World, entity: Entity) {
    if world.get_resource::<DespawnQueue>().is_none() {
        world.insert_resource(DespawnQueue::default());
    }
    world
        .get_resource_mut::<DespawnQueue>()
        .expect("the queue was just inserted")
        .request(entity);
}

/// Applies every pending [`request_despawn`], returning how many entities
/// were destroyed.
///
/// Each live request is removed from the hierarchy (re-rooting its children)
/// and then despawned from the world, in request order. Stale requests —
/// entities that died some other way in between — are skipped. The queue
/// resource is removed; the next [`request_despawn`] recreates it.
pub fn flush_despawns(world: &mut World) -> usize {
    let Some(queue) = world.remove_resource::<DespawnQueue>() else {
        return 0;
    };
    let mut destroyed = 0;
    for entity in queue.requested() {
        if !world.is_alive(entity) {
            continue;
        }
        if let Some(hierarchy) = world.get_resource_mut::<Hierarchy>() {
            // A scene entity is always a node; a hand-spawned entity may not
            // be, and its destruction is still valid.
            let _ = hierarchy.remove(entity);
        }
        if world.despawn(entity) {
            destroyed += 1;
        }
    }
    destroyed
}
