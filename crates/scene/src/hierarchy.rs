//! The entity hierarchy: a forest over [`Entity`] handles, backed by the
//! native scene graph.
//!
//! [`Hierarchy`] is the entity-level wrapper around `mikan-scene-sys`'s safe
//! [`Graph`] handle. It exposes forest semantics over `Entity` values:
//!
//! - every entity has at most one parent; a node with no parent is a root;
//! - a node may not be its own ancestor ([`HierarchyError::Cycle`]);
//! - removing a node re-roots its children — no edge ever dangles;
//! - topology changes never rewrite transforms (see the crate docs).
//!
//! Entity handles are stored packed as `u64` node ids (`Entity::to_bits`),
//! generation included, so a stale handle can never alias a reused slot, and
//! the graph stays free of ECS types.

use core::fmt;

use mikan_ecs::Entity;
use mikan_scene_sys::{Graph, GraphError, Transform2dRepr};
use thiserror::Error;

/// A hierarchy operation could not be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HierarchyError {
    /// The entity is not a node of this hierarchy.
    #[error("entity {0} is not in the hierarchy")]
    UnknownEntity(Entity),
    /// The entity is already a node of this hierarchy.
    #[error("entity {0} is already in the hierarchy")]
    AlreadyPresent(Entity),
    /// The requested parent link would close a cycle.
    #[error("setting {child} under {parent} would create a hierarchy cycle")]
    Cycle {
        /// The node being attached.
        child: Entity,
        /// The proposed parent, which is `child` itself or one of its
        /// descendants.
        parent: Entity,
    },
}

/// The entity forest of a scene: who is the parent of whom.
///
/// Stored as a world resource by [`Scene`](crate::Scene), but usable on its
/// own over any [`World`](mikan_ecs::World) — [`crate::propagate_transforms`]
/// operates on the resource form, so systems can read parent links through
/// `world.get_resource::<Hierarchy>()` while they run.
///
/// ```
/// # use mikan_ecs::Entity;
/// # use mikan_scene::{Hierarchy, HierarchyError};
/// let parent = Entity::from_bits((0_u64 << 32) | 1).unwrap();
/// let child = Entity::from_bits((1_u64 << 32) | 1).unwrap();
/// let mut hierarchy = Hierarchy::new();
/// hierarchy.add(parent)?;
/// hierarchy.add(child)?;
/// hierarchy.set_parent(child, parent)?;
/// assert_eq!(hierarchy.parent(child)?, Some(parent));
/// assert_eq!(hierarchy.children(parent)?, vec![child]);
/// # Ok::<(), HierarchyError>(())
/// ```
pub struct Hierarchy {
    graph: Graph,
}

impl Hierarchy {
    /// Creates an empty hierarchy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
        }
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.graph.len()
    }

    /// Whether there are no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `entity` is a node of this hierarchy.
    #[must_use]
    pub fn contains(&self, entity: Entity) -> bool {
        self.graph.contains(entity.to_bits())
    }

    /// Adds `entity` as a detached root.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::AlreadyPresent`] if the entity is already a node.
    pub fn add(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        match self.graph.add(entity.to_bits()) {
            Ok(()) => Ok(()),
            // The native graph reports a duplicate as a bad argument (the
            // only bad argument reachable here; ids are never 0).
            Err(GraphError::BadArgument) => Err(HierarchyError::AlreadyPresent(entity)),
            Err(_) => Err(HierarchyError::UnknownEntity(entity)),
        }
    }

    /// Removes `entity`, re-rooting its children in place.
    ///
    /// The children keep their local transforms and their relative order;
    /// nothing else about the graph changes.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not a node.
    pub fn remove(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        self.graph
            .remove(entity.to_bits())
            .map_err(|_| HierarchyError::UnknownEntity(entity))
    }

    /// Attaches `child` under `parent`, detaching it from any previous parent.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::Cycle`] if `parent` is `child` or one of its
    /// descendants (the graph is left unchanged); otherwise
    /// [`HierarchyError::UnknownEntity`] naming the entity that is not a node.
    pub fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<(), HierarchyError> {
        match self.graph.set_parent(child.to_bits(), parent.to_bits()) {
            Ok(()) => Ok(()),
            Err(GraphError::Cycle) => Err(HierarchyError::Cycle { child, parent }),
            Err(_) => {
                let unknown = if self.graph.contains(child.to_bits()) {
                    parent
                } else {
                    child
                };
                Err(HierarchyError::UnknownEntity(unknown))
            }
        }
    }

    /// Detaches `entity` from its parent; it becomes a root.
    ///
    /// Detaching a node that is already a root succeeds and changes nothing.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not a node.
    pub fn clear_parent(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        self.graph
            .clear_parent(entity.to_bits())
            .map_err(|_| HierarchyError::UnknownEntity(entity))
    }

    /// The parent of `entity`, or `None` when it is a root.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not a node.
    pub fn parent(&self, entity: Entity) -> Result<Option<Entity>, HierarchyError> {
        match self.graph.parent(entity.to_bits()) {
            Ok(Some(bits)) => Entity::from_bits(bits)
                .map(Some)
                .ok_or(HierarchyError::UnknownEntity(entity)),
            Ok(None) => Ok(None),
            Err(_) => Err(HierarchyError::UnknownEntity(entity)),
        }
    }

    /// The children of `entity`, in attachment order.
    ///
    /// # Errors
    ///
    /// [`HierarchyError::UnknownEntity`] if the entity is not a node.
    pub fn children(&self, entity: Entity) -> Result<Vec<Entity>, HierarchyError> {
        self.graph
            .children(entity.to_bits())
            .map(|ids| ids.into_iter().filter_map(Entity::from_bits).collect())
            .map_err(|_| HierarchyError::UnknownEntity(entity))
    }

    /// Every node, in insertion order (the order entities were added).
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        self.graph
            .nodes()
            .into_iter()
            .filter_map(Entity::from_bits)
            .collect()
    }

    /// Composes world transforms for every node, parents strictly first.
    ///
    /// `locals` and `out` are indexed by [`Hierarchy::entities`] order and
    /// must both hold one entry per node; an entity with no local transform
    /// contributes [`Transform2d::IDENTITY`](crate::Transform2d::IDENTITY) at
    /// the caller's discretion (the scene layer does exactly that).
    ///
    /// This is the crate-internal propagation hook; engine code calls
    /// [`propagate_transforms`](crate::propagate_transforms) instead, which
    /// does the reconciliation and the write-back. Failure means the slices
    /// do not line up with the graph — an internal invariant, not a state
    /// the caller can produce through the public API.
    pub(crate) fn propagate(
        &mut self,
        locals: &[Transform2dRepr],
        out: &mut [Transform2dRepr],
    ) -> Result<(), GraphError> {
        self.graph.propagate(locals, out)
    }
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Hierarchy {
    /// The graph is opaque, so the summary reports what can be observed: the
    /// nodes and their parents.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = formatter.debug_map();
        for entity in self.entities() {
            list.entry(&entity, &self.parent(entity).ok().flatten());
        }
        list.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform2d;

    fn entity(index: u32) -> Entity {
        Entity::from_bits((u64::from(index) << 32) | 1).unwrap()
    }

    #[test]
    fn add_is_unique_and_remove_reroots_children() {
        let (a, b, c) = (entity(0), entity(1), entity(2));
        let mut hierarchy = Hierarchy::new();
        assert!(hierarchy.is_empty());
        hierarchy.add(a).unwrap();
        hierarchy.add(b).unwrap();
        hierarchy.add(c).unwrap();
        assert_eq!(hierarchy.len(), 3);
        assert_eq!(hierarchy.add(a), Err(HierarchyError::AlreadyPresent(a)));
        hierarchy.set_parent(b, a).unwrap();
        hierarchy.set_parent(c, b).unwrap();
        assert_eq!(hierarchy.parent(c).unwrap(), Some(b));

        hierarchy.remove(a).unwrap();
        assert_eq!(hierarchy.parent(b).unwrap(), None, "re-rooted");
        assert_eq!(hierarchy.parent(c).unwrap(), Some(b), "deep link intact");
        assert_eq!(hierarchy.remove(a), Err(HierarchyError::UnknownEntity(a)));
        assert_eq!(hierarchy.entities(), vec![b, c]);
    }

    #[test]
    fn removing_an_inner_node_reroots_and_propagates() {
        // `b` has both a parent (`a`) and a child (`c`). Removing it must
        // detach it from `a` and re-root `c`; a stale edge in `a`'s child
        // list would make propagation revisit `b`, which no longer exists.
        let (a, b, c) = (entity(0), entity(1), entity(2));
        let mut hierarchy = Hierarchy::new();
        for node in [a, b, c] {
            hierarchy.add(node).unwrap();
        }
        hierarchy.set_parent(b, a).unwrap();
        hierarchy.set_parent(c, b).unwrap();

        hierarchy.remove(b).unwrap();
        assert_eq!(hierarchy.entities(), vec![a, c]);
        assert_eq!(hierarchy.children(a).unwrap(), Vec::<Entity>::new());
        assert_eq!(hierarchy.parent(c).unwrap(), None);

        let locals = [
            Transform2d::from_translation([0.0, 1.0]).to_repr(),
            Transform2d::from_translation([0.0, 2.0]).to_repr(),
        ];
        let mut out = [Transform2dRepr::identity(); 2];
        hierarchy.propagate(&locals, &mut out).unwrap();
        assert_eq!(out[0].translation_y, 1.0, "the ex-parent is still a root");
        assert_eq!(out[1].translation_y, 2.0, "the child is a root now");
    }

    #[test]
    fn cycles_are_reported_and_change_nothing() {
        let (a, b) = (entity(0), entity(1));
        let mut hierarchy = Hierarchy::new();
        hierarchy.add(a).unwrap();
        hierarchy.add(b).unwrap();
        hierarchy.set_parent(b, a).unwrap();
        assert_eq!(
            hierarchy.set_parent(a, b),
            Err(HierarchyError::Cycle {
                child: a,
                parent: b
            })
        );
        assert_eq!(
            hierarchy.set_parent(a, a),
            Err(HierarchyError::Cycle {
                child: a,
                parent: a
            })
        );
        assert_eq!(hierarchy.parent(a).unwrap(), None);
        assert_eq!(hierarchy.children(a).unwrap(), vec![b]);
    }

    #[test]
    fn unknown_entities_are_named() {
        let (a, ghost) = (entity(0), entity(9));
        let mut hierarchy = Hierarchy::new();
        hierarchy.add(a).unwrap();
        assert_eq!(
            hierarchy.set_parent(ghost, a),
            Err(HierarchyError::UnknownEntity(ghost))
        );
        assert_eq!(
            hierarchy.set_parent(a, ghost),
            Err(HierarchyError::UnknownEntity(ghost))
        );
        assert_eq!(
            hierarchy.parent(ghost),
            Err(HierarchyError::UnknownEntity(ghost))
        );
        assert_eq!(
            hierarchy.children(ghost),
            Err(HierarchyError::UnknownEntity(ghost))
        );
        assert_eq!(
            hierarchy.remove(ghost),
            Err(HierarchyError::UnknownEntity(ghost))
        );
    }

    #[test]
    fn reparenting_detaches_from_the_old_parent() {
        let (a, b, c) = (entity(0), entity(1), entity(2));
        let mut hierarchy = Hierarchy::new();
        for node in [a, b, c] {
            hierarchy.add(node).unwrap();
        }
        hierarchy.set_parent(c, a).unwrap();
        hierarchy.set_parent(c, b).unwrap();
        assert_eq!(hierarchy.children(a).unwrap(), Vec::<Entity>::new());
        assert_eq!(hierarchy.children(b).unwrap(), vec![c]);
        hierarchy.clear_parent(c).unwrap();
        assert_eq!(hierarchy.parent(c).unwrap(), None);
        hierarchy.clear_parent(c).unwrap();
    }

    #[test]
    fn children_follow_attachment_order() {
        let (a, b, c) = (entity(0), entity(1), entity(2));
        let mut hierarchy = Hierarchy::new();
        for node in [a, b, c] {
            hierarchy.add(node).unwrap();
        }
        hierarchy.set_parent(b, a).unwrap();
        hierarchy.set_parent(c, a).unwrap();
        assert_eq!(hierarchy.children(a).unwrap(), vec![b, c]);
    }

    #[test]
    fn propagation_resolves_parents_before_children() {
        // Insert the grandchild first so insertion order cannot accidentally
        // be the answer.
        let (a, b, c) = (entity(0), entity(1), entity(2));
        let mut hierarchy = Hierarchy::new();
        for node in [c, b, a] {
            hierarchy.add(node).unwrap();
        }
        hierarchy.set_parent(b, a).unwrap();
        hierarchy.set_parent(c, b).unwrap();

        let locals = [
            Transform2d::from_translation([0.0, 3.0]).to_repr(),
            Transform2d::from_translation([0.0, 2.0]).to_repr(),
            Transform2d::from_translation([0.0, 1.0]).to_repr(),
        ];
        let mut out = [Transform2dRepr::identity(); 3];
        hierarchy.propagate(&locals, &mut out).unwrap();
        // entities() order is [c, b, a], and a is the root: a = 1,
        // b = a + b = 3, c = a + b + c = 6.
        assert_eq!(out[0].translation_y, 6.0);
        assert_eq!(out[1].translation_y, 3.0);
        assert_eq!(out[2].translation_y, 1.0);
    }
}
