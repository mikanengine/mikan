//! The safe RAII wrapper around the native scene graph.
//!
//! [`Graph`] owns one opaque `mikan_scene_graph` handle: created exactly once
//! by [`Graph::new`], destroyed exactly once by its `Drop`, and never exposed
//! as a pointer. Its methods translate the C++ module's status codes into
//! [`GraphError`] and uphold every precondition the header states — no null
//! handles, buffers sized before the call, exclusive access through `&mut
//! self`.
//!
//! Node ids are opaque `u64` values to this layer. `mikan-scene` is what
//! gives them entity meaning (it packs `Entity` handles with `to_bits`); the
//! graph itself only knows "node 1 is the parent of node 2" — plus, since
//! these tests are the module's own, whatever the caller passes.
//!
//! The type is neither `Send` nor `Sync`, matching the header's single-thread
//! contract; the raw pointer inside enforces both at compile time.

use core::ptr::NonNull;

use crate::{
    GRAPH_CYCLE, GRAPH_OK, GRAPH_UNKNOWN_NODE, SceneGraph, Transform2dRepr, mikan_scene_graph_add,
    mikan_scene_graph_children, mikan_scene_graph_clear_parent, mikan_scene_graph_contains,
    mikan_scene_graph_create, mikan_scene_graph_destroy, mikan_scene_graph_len,
    mikan_scene_graph_nodes, mikan_scene_graph_parent, mikan_scene_graph_propagate,
    mikan_scene_graph_remove, mikan_scene_graph_set_parent,
};

/// A graph operation failed; see the individual methods for which codes they
/// can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    /// The node id is not in the graph.
    UnknownNode,
    /// The parent link would close a cycle; nothing was changed.
    Cycle,
    /// The argument was rejected: node id 0, a duplicate `add`, or output
    /// slices that do not line up with the graph.
    BadArgument,
}

fn to_graph_error(status: i32) -> GraphError {
    if status == GRAPH_UNKNOWN_NODE {
        GraphError::UnknownNode
    } else if status == GRAPH_CYCLE {
        GraphError::Cycle
    } else {
        GraphError::BadArgument
    }
}

/// An owning handle to one native scene graph.
pub struct Graph {
    handle: NonNull<SceneGraph>,
}

impl Graph {
    /// Creates an empty graph.
    ///
    /// # Panics
    ///
    /// If the native allocation fails — the same failure mode as any other
    /// allocation, reported where it happens rather than threaded through
    /// every caller as a `Result`.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: `create` takes no preconditions and returns either null or
        // a fresh, exclusively-owned handle, which `NonNull::new` filters and
        // `Drop` frees exactly once.
        let handle = unsafe { mikan_scene_graph_create() };
        let Some(handle) = NonNull::new(handle) else {
            panic!("mikan-scene-sys: the native scene graph allocation failed");
        };
        Self { handle }
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: `self.handle` is live for as long as `self` is.
        unsafe { mikan_scene_graph_len(self.handle.as_ptr()) }
    }

    /// Whether there are no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `node` is in the graph.
    #[must_use]
    pub fn contains(&self, node: u64) -> bool {
        // SAFETY: as `len`.
        unsafe { mikan_scene_graph_contains(self.handle.as_ptr(), node) == 1 }
    }

    /// Adds `node` as a detached root.
    ///
    /// # Errors
    ///
    /// [`GraphError::BadArgument`] if `node` is 0 or already present.
    pub fn add(&mut self, node: u64) -> Result<(), GraphError> {
        // SAFETY: `self.handle` is live and `&mut self` proves exclusive
        // access, which is the module's threading contract.
        let status = unsafe { mikan_scene_graph_add(self.handle.as_ptr(), node) };
        if status == GRAPH_OK {
            Ok(())
        } else {
            Err(to_graph_error(status))
        }
    }

    /// Removes `node`, re-rooting its children in place.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownNode`] if `node` is not in the graph.
    pub fn remove(&mut self, node: u64) -> Result<(), GraphError> {
        // SAFETY: as `add`.
        let status = unsafe { mikan_scene_graph_remove(self.handle.as_ptr(), node) };
        if status == GRAPH_OK {
            Ok(())
        } else {
            Err(to_graph_error(status))
        }
    }

    /// Attaches `node` under `parent`, detaching it from any previous parent.
    ///
    /// # Errors
    ///
    /// [`GraphError::Cycle`] if `parent` is `node` or one of its descendants;
    /// [`GraphError::UnknownNode`] if either id is absent;
    /// [`GraphError::BadArgument`] if `parent` is 0.
    pub fn set_parent(&mut self, node: u64, parent: u64) -> Result<(), GraphError> {
        // SAFETY: as `add`.
        let status = unsafe { mikan_scene_graph_set_parent(self.handle.as_ptr(), node, parent) };
        if status == GRAPH_OK {
            Ok(())
        } else {
            Err(to_graph_error(status))
        }
    }

    /// Detaches `node` from its parent; it becomes a root.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownNode`] if `node` is not in the graph.
    pub fn clear_parent(&mut self, node: u64) -> Result<(), GraphError> {
        // SAFETY: as `add`.
        let status = unsafe { mikan_scene_graph_clear_parent(self.handle.as_ptr(), node) };
        if status == GRAPH_OK {
            Ok(())
        } else {
            Err(to_graph_error(status))
        }
    }

    /// The parent node id, or `None` when `node` is a root.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownNode`] if `node` is not in the graph.
    pub fn parent(&self, node: u64) -> Result<Option<u64>, GraphError> {
        let mut parent = 0_u64;
        // SAFETY: `self.handle` is live; `parent` is a live local and the
        // module writes it only on success.
        let status = unsafe { mikan_scene_graph_parent(self.handle.as_ptr(), node, &mut parent) };
        if status != GRAPH_OK {
            return Err(to_graph_error(status));
        }
        Ok((parent != 0).then_some(parent))
    }

    /// The children of `node`, in attachment order.
    ///
    /// # Errors
    ///
    /// [`GraphError::UnknownNode`] if `node` is not in the graph.
    pub fn children(&self, node: u64) -> Result<Vec<u64>, GraphError> {
        // One pass with a worst-case buffer: a node cannot have more children
        // than the graph has nodes.
        let mut buffer = vec![0_u64; self.len()];
        let mut count = 0_usize;
        // SAFETY: `self.handle` is live; the buffer has `self.len()` slots
        // (the maximum possible child count), `count` is a live local, and
        // the module copies at most `capacity` entries and reports the total.
        let status = unsafe {
            mikan_scene_graph_children(
                self.handle.as_ptr(),
                node,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        };
        if status != GRAPH_OK {
            return Err(to_graph_error(status));
        }
        debug_assert!(count <= buffer.len(), "the module never over-reports");
        buffer.truncate(count);
        Ok(buffer)
    }

    /// Every node, in insertion order.
    #[must_use]
    pub fn nodes(&self) -> Vec<u64> {
        let mut buffer = vec![0_u64; self.len()];
        let mut count = 0_usize;
        // SAFETY: `self.handle` is live; the buffer has exactly `self.len()`
        // slots, which is the full node count.
        let status = unsafe {
            mikan_scene_graph_nodes(
                self.handle.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        };
        debug_assert_eq!(status, GRAPH_OK, "sized-to-fit node read cannot fail");
        debug_assert_eq!(count, buffer.len(), "the read is sized to the graph");
        buffer
    }

    /// Composes world transforms for every node, parents strictly first.
    ///
    /// `locals` and `out` are indexed by [`Graph::nodes`] order; both must
    /// hold at least one entry per node.
    ///
    /// # Errors
    ///
    /// [`GraphError::BadArgument`] if either slice is shorter than the node
    /// count; no output is written in that case.
    pub fn propagate(
        &mut self,
        locals: &[Transform2dRepr],
        out: &mut [Transform2dRepr],
    ) -> Result<(), GraphError> {
        let capacity = locals.len().min(out.len());
        let mut count = 0_usize;
        // SAFETY: `self.handle` is live and borrowed exclusively; `locals`
        // and `out` are distinct live slices (they cannot alias: both are
        // borrows held by the caller at once), valid for `capacity` entries.
        // The module rejects `capacity` below the node count before reading,
        // so it can never index past either slice.
        let status = unsafe {
            mikan_scene_graph_propagate(
                self.handle.as_ptr(),
                locals.as_ptr(),
                out.as_mut_ptr(),
                capacity,
                &mut count,
            )
        };
        if status != GRAPH_OK {
            return Err(to_graph_error(status));
        }
        debug_assert_eq!(count, self.len());
        Ok(())
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Graph {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from `create` and is dropped exactly
        // once, here; no method can use it afterwards.
        unsafe { mikan_scene_graph_destroy(self.handle.as_ptr()) };
    }
}

impl core::fmt::Debug for Graph {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Graph")
            .field("nodes", &self.nodes())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_links() {
        let mut graph = Graph::new();
        assert!(graph.is_empty());
        graph.add(1).unwrap();
        graph.add(2).unwrap();
        graph.add(3).unwrap();
        assert_eq!(graph.len(), 3);
        assert!(graph.contains(2));
        assert_eq!(graph.add(2), Err(GraphError::BadArgument));
        assert_eq!(graph.add(0), Err(GraphError::BadArgument));

        graph.set_parent(2, 1).unwrap();
        graph.set_parent(3, 2).unwrap();
        assert_eq!(graph.parent(3).unwrap(), Some(2));
        assert_eq!(graph.parent(1).unwrap(), None);
        assert_eq!(graph.children(1).unwrap(), vec![2]);
        assert_eq!(graph.nodes(), [1, 2, 3]);

        assert_eq!(graph.set_parent(1, 3), Err(GraphError::Cycle));
        assert_eq!(graph.parent(1).unwrap(), None, "unchanged after a cycle");
        assert_eq!(graph.set_parent(9, 1), Err(GraphError::UnknownNode));
        assert_eq!(graph.set_parent(1, 0), Err(GraphError::BadArgument));

        graph.remove(2).unwrap();
        assert_eq!(graph.parent(3).unwrap(), None, "re-rooted");
        assert_eq!(graph.nodes(), [1, 3]);
        assert_eq!(
            graph.children(1).unwrap(),
            Vec::<u64>::new(),
            "the removed node left no stale edge in its parent's children"
        );
        assert_eq!(graph.remove(2), Err(GraphError::UnknownNode));
    }

    #[test]
    fn removing_an_inner_node_reroots_and_propagates() {
        // Removing a node that has both a parent and children must detach it
        // from the parent *and* re-root its children; a stale edge would make
        // `propagate` revisit a node that is no longer in the order.
        let mut graph = Graph::new();
        graph.add(1).unwrap();
        graph.add(2).unwrap();
        graph.add(3).unwrap();
        graph.set_parent(2, 1).unwrap();
        graph.set_parent(3, 2).unwrap();
        graph.remove(2).unwrap();

        assert_eq!(graph.nodes(), [1, 3]);
        assert_eq!(graph.parent(3).unwrap(), None);
        assert_eq!(graph.children(1).unwrap(), Vec::<u64>::new());

        let locals = [
            Transform2dRepr {
                translation_x: 1.0,
                ..Transform2dRepr::identity()
            },
            Transform2dRepr {
                translation_x: 2.0,
                ..Transform2dRepr::identity()
            },
        ];
        let mut out = [Transform2dRepr::identity(); 2];
        graph.propagate(&locals, &mut out).unwrap();
        assert_eq!(out[0].translation_x, 1.0, "the ex-parent is still a root");
        assert_eq!(out[1].translation_x, 2.0, "the child is a root now");
    }

    #[test]
    fn propagate_composes_in_dependency_order() {
        let mut graph = Graph::new();
        graph.add(2).unwrap();
        graph.add(1).unwrap();
        graph.set_parent(2, 1).unwrap();
        let locals = [
            Transform2dRepr {
                translation_x: 3.0,
                ..Transform2dRepr::identity()
            },
            Transform2dRepr {
                translation_x: 10.0,
                ..Transform2dRepr::identity()
            },
        ];
        let mut out = [Transform2dRepr::identity(); 2];
        graph.propagate(&locals, &mut out).unwrap();
        assert_eq!(out[1], locals[1]);
        assert_eq!(out[0].translation_x, 13.0);

        assert_eq!(
            graph.propagate(&locals[..1], &mut out),
            Err(GraphError::BadArgument)
        );
    }

    #[test]
    fn short_children_read_is_not_special() {
        let mut graph = Graph::new();
        graph.add(7).unwrap();
        assert_eq!(graph.children(7).unwrap(), Vec::<u64>::new());
        assert_eq!(graph.children(8), Err(GraphError::UnknownNode));
    }
}
