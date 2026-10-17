//! Raw interface to the native scene module (`mikan_scene`).
//!
//! This crate is the ABI mirror of `src/mikan_transform.h` and
//! `src/mikan_scene_graph.h`: the `#[repr(C)]` transform struct, the status
//! codes, the `extern "C"` declarations for the C++ graph, and one safe entry
//! point for the C compose kernel. `mikan-scene` is the crate applications
//! should depend on; it wraps the opaque graph in the safe `Hierarchy` type
//! and offers the independent Rust reference used to cross-check the kernel.
//!
//! # Why `unsafe` is allowed in this crate
//!
//! The workspace forbids `unsafe_code` crate-wide. This is the crate that
//! bridges into the native scene module, and the rules that make the calls
//! sound — opaque handle validity, buffer lengths, non-aliasing, exclusive
//! access to a graph — are a C ABI contract the Rust type system cannot check.
//! The override is recorded here, per `docs/ffi.md`; every occurrence carries
//! a `// SAFETY:` comment; the safe wrapper (`mikan-scene`) and every consumer
//! contain none.
//!
//! # Layout and ownership
//!
//! [`Transform2dRepr`] is plain data read and written by value across the
//! boundary; neither side retains a pointer to it. A
//! [`SceneGraph`] handle is owned by the caller: [`mikan_scene_graph_create`]
//! allocates it, [`mikan_scene_graph_destroy`] frees it, and it must be
//! destroyed exactly once per successful create. The graph is not
//! thread-safe: every call that takes a non-const handle requires exclusive
//! access, which the safe wrapper encodes as `&mut self`.

use core::ffi::c_int;

mod graph;

pub use graph::{Graph, GraphError};

/// A parent-relative or world-space 2D transform (translation, rotation,
/// scale), layout-identical to `mikan_transform2d` in `src/mikan_transform.h`.
///
/// `rotation` is in radians and is the source of truth; `(cos_rotation,
/// sin_rotation)` must be the unit pair `(cos(rotation), sin(rotation))`.
/// Rust code builds values through the checked constructors in `mikan-scene`
/// so the invariant holds; the kernel preserves it exactly through
/// composition because it never computes trigonometry itself.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2dRepr {
    /// X component of the translation.
    pub translation_x: f32,
    /// Y component of the translation.
    pub translation_y: f32,
    /// Rotation in radians.
    pub rotation: f32,
    /// X component of the scale.
    pub scale_x: f32,
    /// Y component of the scale.
    pub scale_y: f32,
    /// `cos(rotation)`, cached to keep composition trigonometry-free.
    pub cos_rotation: f32,
    /// `sin(rotation)`, cached to keep composition trigonometry-free.
    pub sin_rotation: f32,
}

impl Transform2dRepr {
    /// The identity transform: no translation, no rotation, unit scale.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            translation_x: 0.0,
            translation_y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            cos_rotation: 1.0,
            sin_rotation: 0.0,
        }
    }
}

/// `MIKAN_GRAPH_OK`: the call succeeded.
pub const GRAPH_OK: c_int = 0;
/// `MIKAN_GRAPH_UNKNOWN_NODE`: the node id is not in the graph.
pub const GRAPH_UNKNOWN_NODE: c_int = 1;
/// `MIKAN_GRAPH_CYCLE`: the parent link would close a cycle.
pub const GRAPH_CYCLE: c_int = 2;
/// `MIKAN_GRAPH_BAD_ARGUMENT`: a null pointer, id 0, or a duplicate/absent
/// precondition (depending on the function).
pub const GRAPH_BAD_ARGUMENT: c_int = 3;

/// An opaque `mikan_scene_graph` handle; see the module docs.
#[repr(C)]
pub struct SceneGraph {
    _private: [u8; 0],
}

unsafe extern "C" {
    /// The raw C compose kernel, exposed for `-sys`-style completeness and
    /// defensive tests; prefer [`compose`], which is safe.
    ///
    /// # Safety
    ///
    /// `parent`, `local`, and `out` must be valid, aligned, non-null
    /// pointers to [`Transform2dRepr`] values. `out` may alias either input;
    /// the kernel reads both fully before writing.
    pub fn mikan_transform2d_compose(
        parent: *const Transform2dRepr,
        local: *const Transform2dRepr,
        out: *mut Transform2dRepr,
    );

    /// Allocates an empty graph; returns null on allocation failure.
    pub fn mikan_scene_graph_create() -> *mut SceneGraph;

    /// Frees a graph created by [`mikan_scene_graph_create`]; null is a no-op.
    ///
    /// # Safety
    ///
    /// `graph` must be null or a handle from [`mikan_scene_graph_create`] that
    /// has not been destroyed; after this call it must not be used again.
    pub fn mikan_scene_graph_destroy(graph: *mut SceneGraph);

    /// Number of nodes; 0 for a null graph.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live.
    pub fn mikan_scene_graph_len(graph: *const SceneGraph) -> usize;

    /// 1 if `node` exists, 0 otherwise (including a null graph or node 0).
    ///
    /// # Safety
    ///
    /// `graph` must be null or live.
    pub fn mikan_scene_graph_contains(graph: *const SceneGraph, node: u64) -> c_int;

    /// Adds a detached node; returns a status code.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live, and must not be accessed concurrently.
    pub fn mikan_scene_graph_add(graph: *mut SceneGraph, node: u64) -> c_int;

    /// Removes a node, re-rooting its children; returns a status code.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live, and must not be accessed concurrently.
    pub fn mikan_scene_graph_remove(graph: *mut SceneGraph, node: u64) -> c_int;

    /// Attaches `node` under `parent`; returns a status code.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live, and must not be accessed concurrently.
    pub fn mikan_scene_graph_set_parent(graph: *mut SceneGraph, node: u64, parent: u64) -> c_int;

    /// Detaches `node` from its parent; returns a status code.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live, and must not be accessed concurrently.
    pub fn mikan_scene_graph_clear_parent(graph: *mut SceneGraph, node: u64) -> c_int;

    /// Reads `node`'s parent (0 when detached) into `out_parent`.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live and `out_parent` a valid writable pointer.
    pub fn mikan_scene_graph_parent(
        graph: *const SceneGraph,
        node: u64,
        out_parent: *mut u64,
    ) -> c_int;

    /// Copies up to `capacity` children of `node` and reports the full count.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live; `out_children` must be valid for
    /// `capacity` writes (`capacity` may be 0); `out_count` must be a valid
    /// writable pointer.
    pub fn mikan_scene_graph_children(
        graph: *const SceneGraph,
        node: u64,
        out_children: *mut u64,
        capacity: usize,
        out_count: *mut usize,
    ) -> c_int;

    /// Copies up to `capacity` nodes in insertion order and reports the full
    /// count.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live; `out_nodes` must be valid for `capacity`
    /// writes (`capacity` may be 0); `out_count` must be a valid writable
    /// pointer.
    pub fn mikan_scene_graph_nodes(
        graph: *const SceneGraph,
        out_nodes: *mut u64,
        capacity: usize,
        out_count: *mut usize,
    ) -> c_int;

    /// Composes world transforms for every node in dependency order.
    ///
    /// # Safety
    ///
    /// `graph` must be null or live and not accessed concurrently; `locals`
    /// and `out` must be valid for `capacity` reads/writes and must not
    /// alias; `out_count` must be a valid writable pointer.
    pub fn mikan_scene_graph_propagate(
        graph: *mut SceneGraph,
        locals: *const Transform2dRepr,
        out: *mut Transform2dRepr,
        capacity: usize,
        out_count: *mut usize,
    ) -> c_int;
}

/// Composes `parent` with `local` (`out = parent ∘ local`), exactly as the C
/// kernel does.
///
/// This is a by-value wrapper: the result is a fresh value, so no aliasing
/// question exists and the call cannot panic or fail.
#[must_use]
pub fn compose(parent: &Transform2dRepr, local: &Transform2dRepr) -> Transform2dRepr {
    let mut out = Transform2dRepr::identity();
    // SAFETY: `parent` and `local` are live references and `out` is a live
    // local; all three are distinct by construction (the compiler moves `out`
    // into this frame), so the kernel's non-aliasing contract holds. The
    // kernel writes one `Transform2dRepr` through `out` and retains no
    // pointer.
    unsafe {
        mikan_transform2d_compose(parent, local, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_composition_is_the_other_operand() {
        let t = Transform2dRepr {
            translation_x: 3.5,
            translation_y: -1.25,
            rotation: 0.75,
            scale_x: 2.0,
            scale_y: 0.5,
            cos_rotation: 0.75f32.cos(),
            sin_rotation: 0.75f32.sin(),
        };
        assert_eq!(compose(&Transform2dRepr::identity(), &t), t);
        assert_eq!(compose(&t, &Transform2dRepr::identity()), t);
    }

    #[test]
    fn rotation_composes_by_exact_addition() {
        let parent = Transform2dRepr {
            rotation: 1.0,
            cos_rotation: 1.0f32.cos(),
            sin_rotation: 1.0f32.sin(),
            ..Transform2dRepr::identity()
        };
        let local = Transform2dRepr {
            rotation: 2.0,
            cos_rotation: 2.0f32.cos(),
            sin_rotation: 2.0f32.sin(),
            ..Transform2dRepr::identity()
        };
        assert_eq!(compose(&parent, &local).rotation, 3.0);
    }
}
