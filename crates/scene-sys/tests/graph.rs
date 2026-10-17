//! Exercises the raw `mikan_scene_graph` ABI: status codes, defensive null
//! handling, insertion order, cycle rejection, re-rooting, and the
//! dependency-ordered propagation contract. The safe wrapper (`mikan-scene`)
//! has its own semantic tests; these prove the layer underneath.

use mikan_scene_sys::{
    GRAPH_BAD_ARGUMENT, GRAPH_CYCLE, GRAPH_OK, GRAPH_UNKNOWN_NODE, SceneGraph, Transform2dRepr,
    compose, mikan_scene_graph_add, mikan_scene_graph_children, mikan_scene_graph_clear_parent,
    mikan_scene_graph_contains, mikan_scene_graph_create, mikan_scene_graph_destroy,
    mikan_scene_graph_len, mikan_scene_graph_nodes, mikan_scene_graph_parent,
    mikan_scene_graph_propagate, mikan_scene_graph_remove, mikan_scene_graph_set_parent,
    mikan_transform2d_compose,
};
use std::ptr;

struct RawGraph(*mut SceneGraph);

impl RawGraph {
    fn new() -> Self {
        let handle = unsafe {
            // SAFETY: no precondition beyond a normal allocation; the handle
            // is checked for null immediately and freed in `Drop`.
            mikan_scene_graph_create()
        };
        assert!(!handle.is_null(), "allocation failed");
        Self(handle)
    }

    fn len(&self) -> usize {
        unsafe {
            // SAFETY: `self.0` came from `create` and is freed only in `Drop`.
            mikan_scene_graph_len(self.0)
        }
    }

    fn add(&self, node: u64) -> i32 {
        unsafe {
            // SAFETY: live handle; this borrow keeps access exclusive in
            // practice (the test never shares the graph across threads).
            mikan_scene_graph_add(self.0, node)
        }
    }

    fn remove(&self, node: u64) -> i32 {
        unsafe {
            // SAFETY: live handle, as above.
            mikan_scene_graph_remove(self.0, node)
        }
    }

    fn set_parent(&self, node: u64, parent: u64) -> i32 {
        unsafe {
            // SAFETY: live handle, as above.
            mikan_scene_graph_set_parent(self.0, node, parent)
        }
    }

    fn clear_parent(&self, node: u64) -> i32 {
        unsafe {
            // SAFETY: live handle, as above.
            mikan_scene_graph_clear_parent(self.0, node)
        }
    }

    fn parent_of(&self, node: u64) -> i32 {
        let mut parent = u64::MAX;
        let status = unsafe {
            // SAFETY: live handle and a live local out-pointer.
            mikan_scene_graph_parent(self.0, node, &mut parent)
        };
        if status == GRAPH_OK {
            assert!(parent <= i32::MAX as u64, "test ids are small");
            parent as i32
        } else {
            status
        }
    }

    fn children_of(&self, node: u64, capacity: usize) -> (i32, Vec<u64>, usize) {
        let mut buffer = vec![0u64; capacity];
        let mut count = usize::MAX;
        let status = unsafe {
            // SAFETY: live handle; the buffer is valid for `capacity` writes
            // (possibly zero) and `count` is a live local.
            mikan_scene_graph_children(self.0, node, buffer.as_mut_ptr(), capacity, &mut count)
        };
        (status, buffer, count)
    }

    fn nodes(&self) -> Vec<u64> {
        let mut buffer = vec![0u64; self.len()];
        let mut count = 0;
        let status = unsafe {
            // SAFETY: live handle; the buffer has exactly `len()` slots and
            // `count` is a live local.
            mikan_scene_graph_nodes(self.0, buffer.as_mut_ptr(), buffer.len(), &mut count)
        };
        assert_eq!(status, GRAPH_OK);
        assert_eq!(count, buffer.len());
        buffer
    }
}

impl Drop for RawGraph {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: `self.0` is a live handle from `create` and is dropped
            // exactly once, here.
            mikan_scene_graph_destroy(self.0);
        }
    }
}

fn local(x: f32, y: f32) -> Transform2dRepr {
    Transform2dRepr {
        translation_x: x,
        translation_y: y,
        ..Transform2dRepr::identity()
    }
}

#[test]
fn add_rejects_zero_and_duplicates() {
    let graph = RawGraph::new();
    assert_eq!(graph.len(), 0);
    assert_eq!(graph.add(7), GRAPH_OK);
    assert_eq!(graph.add(7), GRAPH_BAD_ARGUMENT, "duplicate");
    assert_eq!(graph.add(0), GRAPH_BAD_ARGUMENT, "0 is the sentinel");
    assert_eq!(graph.len(), 1);
    let contains = unsafe {
        // SAFETY: live handle.
        mikan_scene_graph_contains(graph.0, 7)
    };
    assert_eq!(contains, 1);
    let contains_zero = unsafe {
        // SAFETY: live handle.
        mikan_scene_graph_contains(graph.0, 0)
    };
    assert_eq!(contains_zero, 0);
}

#[test]
fn null_graph_is_rejected_not_crashed() {
    assert_eq!(
        unsafe {
            // SAFETY: the documented contract explicitly tolerates null.
            mikan_scene_graph_len(ptr::null())
        },
        0
    );
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance, as above.
            mikan_scene_graph_contains(ptr::null(), 1)
        },
        0
    );
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance, as above.
            mikan_scene_graph_add(ptr::null_mut(), 1)
        },
        GRAPH_BAD_ARGUMENT
    );
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance, as above.
            mikan_scene_graph_remove(ptr::null_mut(), 1)
        },
        GRAPH_UNKNOWN_NODE
    );
    let mut parent = u64::MAX;
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance; `parent` is a live local.
            mikan_scene_graph_parent(ptr::null(), 1, &mut parent)
        },
        GRAPH_UNKNOWN_NODE
    );
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance; out pointer is deliberately null too.
            mikan_scene_graph_parent(ptr::null(), 1, ptr::null_mut())
        },
        GRAPH_BAD_ARGUMENT
    );
    let mut count = 0;
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance; `count` is a live local.
            mikan_scene_graph_nodes(ptr::null(), ptr::null_mut(), 0, &mut count)
        },
        GRAPH_BAD_ARGUMENT
    );
    assert_eq!(
        unsafe {
            // SAFETY: null tolerance; all output pointers are null.
            mikan_scene_graph_propagate(
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                0,
                &mut count,
            )
        },
        GRAPH_BAD_ARGUMENT
    );
    unsafe {
        // SAFETY: destroying null is documented as a no-op.
        mikan_scene_graph_destroy(ptr::null_mut());
    }
}

#[test]
fn links_are_readable_and_insertion_order_is_stable() {
    let graph = RawGraph::new();
    assert_eq!(graph.add(1), GRAPH_OK);
    assert_eq!(graph.add(2), GRAPH_OK);
    assert_eq!(graph.add(3), GRAPH_OK);
    assert_eq!(graph.nodes(), [1, 2, 3]);

    assert_eq!(graph.set_parent(2, 1), GRAPH_OK);
    assert_eq!(graph.set_parent(3, 2), GRAPH_OK);
    assert_eq!(graph.parent_of(1), 0, "detached");
    assert_eq!(graph.parent_of(2), 1);
    assert_eq!(graph.parent_of(3), 2);

    let (status, buffer, count) = graph.children_of(1, 4);
    assert_eq!(status, GRAPH_OK);
    assert_eq!(count, 1);
    assert_eq!(&buffer[..1], [2]);
    assert_eq!(graph.parent_of(99), GRAPH_UNKNOWN_NODE);

    assert_eq!(graph.clear_parent(3), GRAPH_OK);
    assert_eq!(graph.parent_of(3), 0);
    assert_eq!(graph.clear_parent(3), GRAPH_OK, "clearing again is a no-op");
}

#[test]
fn children_copies_min_capacity_and_reports_full_count() {
    let graph = RawGraph::new();
    for node in [1, 2, 3] {
        assert_eq!(graph.add(node), GRAPH_OK);
    }
    assert_eq!(graph.set_parent(2, 1), GRAPH_OK);
    assert_eq!(graph.set_parent(3, 1), GRAPH_OK);
    let (status, buffer, count) = graph.children_of(1, 1);
    assert_eq!(status, GRAPH_OK);
    assert_eq!(count, 2, "full count regardless of capacity");
    assert_eq!(&buffer[..1], [2]);

    let mut count = usize::MAX;
    let status = unsafe {
        // SAFETY: live handle; a null buffer with capacity 0 is allowed.
        mikan_scene_graph_children(graph.0, 1, ptr::null_mut(), 0, &mut count)
    };
    assert_eq!(status, GRAPH_OK);
    assert_eq!(count, 2);
}

#[test]
fn cycles_are_rejected_without_mutating_the_graph() {
    let graph = RawGraph::new();
    for node in [1, 2, 3] {
        assert_eq!(graph.add(node), GRAPH_OK);
    }
    assert_eq!(graph.set_parent(2, 1), GRAPH_OK);
    assert_eq!(graph.set_parent(3, 2), GRAPH_OK);

    assert_eq!(graph.set_parent(1, 1), GRAPH_CYCLE, "self-parent");
    assert_eq!(graph.set_parent(1, 2), GRAPH_CYCLE, "direct descendant");
    assert_eq!(graph.set_parent(1, 3), GRAPH_CYCLE, "deep descendant");
    assert_eq!(graph.parent_of(1), 0, "rejections must not mutate");
    assert_eq!(graph.set_parent(4, 1), GRAPH_UNKNOWN_NODE);
    assert_eq!(graph.set_parent(1, 4), GRAPH_UNKNOWN_NODE);
    assert_eq!(
        graph.set_parent(1, 0),
        GRAPH_BAD_ARGUMENT,
        "use clear_parent"
    );
    assert_eq!(graph.parent_of(2), 1);
}

#[test]
fn remove_reroots_children_and_preserves_relative_order() {
    let graph = RawGraph::new();
    for node in [1, 2, 3, 4] {
        assert_eq!(graph.add(node), GRAPH_OK);
    }
    assert_eq!(graph.set_parent(2, 1), GRAPH_OK);
    assert_eq!(graph.set_parent(3, 1), GRAPH_OK);
    assert_eq!(graph.set_parent(4, 2), GRAPH_OK);

    assert_eq!(graph.remove(1), GRAPH_OK);
    assert_eq!(graph.remove(1), GRAPH_UNKNOWN_NODE);
    assert_eq!(graph.nodes(), [2, 3, 4], "removal keeps insertion order");
    assert_eq!(graph.parent_of(2), 0, "children become roots");
    assert_eq!(graph.parent_of(3), 0);
    assert_eq!(graph.parent_of(4), 2, "grandchild link survives");
    assert_eq!(graph.len(), 3);
}

#[test]
fn propagate_composes_parents_before_children_in_any_insertion_order() {
    let graph = RawGraph::new();
    assert_eq!(graph.add(2), GRAPH_OK, "child first in insertion order");
    assert_eq!(graph.add(1), GRAPH_OK, "parent second");
    assert_eq!(graph.set_parent(2, 1), GRAPH_OK);

    let locals = [local(3.0, 4.0), local(10.0, 0.0)];
    let mut out = [Transform2dRepr::identity(); 2];
    let mut count = 0;
    let status = unsafe {
        // SAFETY: live handle (this call takes it mutably but access is
        // exclusive); `locals` and `out` are distinct live arrays with the
        // graph's length.
        mikan_scene_graph_propagate(graph.0, locals.as_ptr(), out.as_mut_ptr(), 2, &mut count)
    };
    assert_eq!(status, GRAPH_OK);
    assert_eq!(count, 2);
    assert_eq!(out[1], locals[1], "root copied verbatim");
    assert_eq!(out[0].translation_x, 13.0, "child composed with parent");
    assert_eq!(out[0].translation_y, 4.0);

    let short = {
        let mut count = 0;
        unsafe {
            // SAFETY: live handle; `locals`/`out` are live but capacity is
            // deliberately one below the node count.
            mikan_scene_graph_propagate(graph.0, locals.as_ptr(), out.as_mut_ptr(), 1, &mut count)
        }
    };
    assert_eq!(short, GRAPH_BAD_ARGUMENT);
}

#[test]
fn compose_allows_aliasing_and_computes_the_fixed_grouping() {
    let parent = Transform2dRepr {
        translation_x: 5.0,
        translation_y: -2.0,
        rotation: 0.5,
        scale_x: 2.0,
        scale_y: 3.0,
        cos_rotation: 0.5f32.cos(),
        sin_rotation: 0.5f32.sin(),
    };
    let child = local(1.5, -0.5);

    let expected = compose(&parent, &child);
    let mut aliased = parent;
    unsafe {
        // SAFETY: all three pointers are live and aligned; `out` aliases
        // `parent`, which the kernel explicitly supports by reading every
        // input before writing.
        mikan_transform2d_compose(&parent, &child, &mut aliased);
    }
    assert_eq!(aliased, expected, "aliasing out with parent is supported");
    assert_eq!(aliased.rotation, 0.5);
    assert_eq!(aliased.scale_x, 2.0);
    assert_eq!(aliased.scale_y, 3.0);
}
