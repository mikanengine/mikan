/*
 * Mikan scene graph — the low-level topology layer of the scene system.
 *
 * This header is the contract; the implementation lives in scene_graph.cpp,
 * the Rust RAII wrapper in crates/scene-sys/src/lib.rs, and the semantic tests
 * in crates/scene/tests/.
 *
 * The graph stores entity identities as opaque u64 handles (the Rust side
 * packs an Entity index and generation into one value) and answers three
 * questions:
 *
 *   1. Topology: which node is the parent of which child, in insertion order.
 *   2. Validity: a hierarchy must stay a forest — every node has at most one
 *      parent, no node is its own ancestor, and no dangling node references
 *      survive an entity's removal.
 *   3. Order: the dependency-safe order in which local transforms must be
 *      composed into world transforms (parents strictly before children).
 *
 * Semantics:
 *
 *   - Node id 0 is reserved as the "no parent" sentinel and is rejected as a
 *     node id by every function.
 *   - The graph owns only topology: it never stores transforms. The C++ layer
 *     composes nothing itself — mikan_scene_graph_propagate walks the nodes in
 *     dependency order and delegates all arithmetic to the C kernel in
 *     mikan_transform.h.
 *   - Insertion order is stable and observable through mikan_scene_graph_nodes;
 *     propagation uses that same order for its index space, so callers can
 *     build the `locals` array from one `nodes` call and trust it to line up.
 *   - Removing a node re-roots its children: they keep their local transforms
 *     and simply lose their parent, so no dangling edges can outlive an entity.
 *   - Every function validates its arguments and returns a status code; null
 *     pointers are tolerated (except where an output is required) and reported
 *     as MIKAN_GRAPH_BAD_ARGUMENT or MIKAN_GRAPH_UNKNOWN_NODE, never as a
 *     crash. Allocation failure is fatal (the module is built without
 *     exception support); nothing throws across this boundary.
 *   - Not thread-safe: a graph instance requires exclusive access, exactly as
 *     its Rust wrapper (Hierarchy) does through &mut self.
 */
#ifndef MIKAN_SCENE_GRAPH_H
#define MIKAN_SCENE_GRAPH_H

#include <stddef.h>
#include <stdint.h>

#include "mikan_transform.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct mikan_scene_graph mikan_scene_graph;

enum {
    MIKAN_GRAPH_OK = 0,
    MIKAN_GRAPH_UNKNOWN_NODE = 1,
    MIKAN_GRAPH_CYCLE = 2,
    MIKAN_GRAPH_BAD_ARGUMENT = 3
};

/* Creates an empty graph; returns NULL on allocation failure. */
mikan_scene_graph *mikan_scene_graph_create(void);

/* Destroys a graph; NULL is a no-op. */
void mikan_scene_graph_destroy(mikan_scene_graph *graph);

/* Number of nodes; 0 for a NULL graph. */
size_t mikan_scene_graph_len(const mikan_scene_graph *graph);

/* 1 if `node` exists, 0 otherwise (including a NULL graph or node 0). */
int mikan_scene_graph_contains(const mikan_scene_graph *graph, uint64_t node);

/* Adds a detached node. MIKAN_GRAPH_BAD_ARGUMENT if node is 0 or already
 * present. */
int mikan_scene_graph_add(mikan_scene_graph *graph, uint64_t node);

/* Removes a node. Its children become roots and keep their local transforms
 * (the Rust scene layer bakes world placement before removal when it wants
 * to). MIKAN_GRAPH_UNKNOWN_NODE if node is absent. */
int mikan_scene_graph_remove(mikan_scene_graph *graph, uint64_t node);

/* Attaches `node` under an existing `parent`. Detaches from any previous
 * parent first. MIKAN_GRAPH_CYCLE if parent is node or one of its
 * descendants; MIKAN_GRAPH_BAD_ARGUMENT if parent is 0 (use clear_parent) or
 * node is 0; MIKAN_GRAPH_UNKNOWN_NODE if either id is absent. */
int mikan_scene_graph_set_parent(mikan_scene_graph *graph, uint64_t node,
                                 uint64_t parent);

/* Detaches `node` from its parent (if any). MIKAN_GRAPH_UNKNOWN_NODE if node
 * is absent. */
int mikan_scene_graph_clear_parent(mikan_scene_graph *graph, uint64_t node);

/* Reads `node`'s parent into `*out_parent` (0 when detached). */
int mikan_scene_graph_parent(const mikan_scene_graph *graph, uint64_t node,
                             uint64_t *out_parent);

/* Copies up to `capacity` children of `node` into `out_children` in insertion
 * order and always reports the full child count through `out_count`. */
int mikan_scene_graph_children(const mikan_scene_graph *graph, uint64_t node,
                               uint64_t *out_children, size_t capacity,
                               size_t *out_count);

/* Copies up to `capacity` nodes into `out_nodes` in insertion order and always
 * reports the full node count through `out_count`. */
int mikan_scene_graph_nodes(const mikan_scene_graph *graph, uint64_t *out_nodes,
                            size_t capacity, size_t *out_count);

/* Composes world transforms for every node, in an order where each parent is
 * resolved before its children. `locals`, `out` and the graph's insertion
 * order share one index space: entry i of `locals` is the local transform of
 * node i, and entry i of `out` receives its world transform. Roots are copied
 * unchanged; every other node is composed with its parent through
 * mikan_transform2d_compose.
 *
 * Returns MIKAN_GRAPH_BAD_ARGUMENT if `locals`, `out` or `out_count` is null
 * or `capacity` is smaller than the node count. `out` must not alias
 * `locals`. */
int mikan_scene_graph_propagate(mikan_scene_graph *graph,
                                const mikan_transform2d *locals,
                                mikan_transform2d *out, size_t capacity,
                                size_t *out_count);

#ifdef __cplusplus
}
#endif

#endif /* MIKAN_SCENE_GRAPH_H */
