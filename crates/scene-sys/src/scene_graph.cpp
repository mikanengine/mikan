#include "mikan_scene_graph.h"

#include <new>
#include <unordered_map>
#include <vector>

struct mikan_scene_graph {
    /* Node ids in insertion order; this order is the index space shared with
     * mikan_scene_graph_nodes and mikan_scene_graph_propagate. */
    std::vector<uint64_t> order;
    /* Every node is present; value 0 means "no parent". */
    std::unordered_map<uint64_t, uint64_t> parent;
    /* Every node is present, even when detached, so children lists are stable. */
    std::unordered_map<uint64_t, std::vector<uint64_t> > children;
};

namespace {

uint64_t parent_of(const mikan_scene_graph *graph, uint64_t node) {
    std::unordered_map<uint64_t, uint64_t>::const_iterator it =
        graph->parent.find(node);
    return it == graph->parent.end() ? 0 : it->second;
}

bool known(const mikan_scene_graph *graph, uint64_t node) {
    return graph != NULL && node != 0 && graph->parent.find(node) != graph->parent.end();
}

void detach(mikan_scene_graph *graph, uint64_t node) {
    std::unordered_map<uint64_t, uint64_t>::iterator pit =
        graph->parent.find(node);
    if (pit == graph->parent.end() || pit->second == 0) {
        return;
    }
    std::unordered_map<uint64_t, std::vector<uint64_t> >::iterator cit =
        graph->children.find(pit->second);
    if (cit != graph->children.end()) {
        std::vector<uint64_t> &siblings = cit->second;
        for (size_t i = 0; i < siblings.size(); ++i) {
            if (siblings[i] == node) {
                siblings.erase(siblings.begin() + static_cast<ptrdiff_t>(i));
                break;
            }
        }
    }
    pit->second = 0;
}

}  // namespace

mikan_scene_graph *mikan_scene_graph_create(void) {
    return new (std::nothrow) mikan_scene_graph();
}

void mikan_scene_graph_destroy(mikan_scene_graph *graph) {
    delete graph;
}

size_t mikan_scene_graph_len(const mikan_scene_graph *graph) {
    return graph == NULL ? 0 : graph->order.size();
}

int mikan_scene_graph_contains(const mikan_scene_graph *graph, uint64_t node) {
    return known(graph, node) ? 1 : 0;
}

int mikan_scene_graph_add(mikan_scene_graph *graph, uint64_t node) {
    if (known(graph, node)) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    if (graph == NULL || node == 0) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    graph->parent[node] = 0;
    graph->children[node] = std::vector<uint64_t>();
    graph->order.push_back(node);
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_remove(mikan_scene_graph *graph, uint64_t node) {
    if (!known(graph, node)) {
        return MIKAN_GRAPH_UNKNOWN_NODE;
    }
    /* Detach from the parent first: propagation walks the children lists, not
     * the parent links, so a stale entry there would make it revisit a node
     * that is no longer in `order` — and never terminate. */
    detach(graph, node);
    /* Children keep their local transforms and become roots: no dangling
     * edges survive the removal. */
    std::unordered_map<uint64_t, std::vector<uint64_t> >::iterator cit =
        graph->children.find(node);
    if (cit != graph->children.end()) {
        for (size_t i = 0; i < cit->second.size(); ++i) {
            std::unordered_map<uint64_t, uint64_t>::iterator pit =
                graph->parent.find(cit->second[i]);
            if (pit != graph->parent.end()) {
                pit->second = 0;
            }
        }
        graph->children.erase(cit);
    }
    graph->parent.erase(node);
    for (size_t i = 0; i < graph->order.size(); ++i) {
        if (graph->order[i] == node) {
            graph->order.erase(graph->order.begin() + static_cast<ptrdiff_t>(i));
            break;
        }
    }
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_set_parent(mikan_scene_graph *graph, uint64_t node,
                                 uint64_t parent) {
    if (node == 0 || parent == 0) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    if (!known(graph, node) || !known(graph, parent)) {
        return MIKAN_GRAPH_UNKNOWN_NODE;
    }
    /* Reject cycles before mutating anything: walking up from the prospective
     * parent must never reach the node itself. */
    uint64_t ancestor = parent;
    while (ancestor != 0) {
        if (ancestor == node) {
            return MIKAN_GRAPH_CYCLE;
        }
        ancestor = parent_of(graph, ancestor);
    }
    detach(graph, node);
    graph->parent[node] = parent;
    graph->children[parent].push_back(node);
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_clear_parent(mikan_scene_graph *graph, uint64_t node) {
    if (!known(graph, node)) {
        return MIKAN_GRAPH_UNKNOWN_NODE;
    }
    detach(graph, node);
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_parent(const mikan_scene_graph *graph, uint64_t node,
                             uint64_t *out_parent) {
    if (out_parent == NULL) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    if (!known(graph, node)) {
        return MIKAN_GRAPH_UNKNOWN_NODE;
    }
    *out_parent = parent_of(graph, node);
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_children(const mikan_scene_graph *graph, uint64_t node,
                               uint64_t *out_children, size_t capacity,
                               size_t *out_count) {
    if (out_count == NULL || (capacity > 0 && out_children == NULL)) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    *out_count = 0;
    if (!known(graph, node)) {
        return MIKAN_GRAPH_UNKNOWN_NODE;
    }
    std::unordered_map<uint64_t, std::vector<uint64_t> >::const_iterator cit =
        graph->children.find(node);
    if (cit == graph->children.end()) {
        return MIKAN_GRAPH_OK;
    }
    const std::vector<uint64_t> &kids = cit->second;
    *out_count = kids.size();
    size_t copied = kids.size() < capacity ? kids.size() : capacity;
    for (size_t i = 0; i < copied; ++i) {
        out_children[i] = kids[i];
    }
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_nodes(const mikan_scene_graph *graph, uint64_t *out_nodes,
                            size_t capacity, size_t *out_count) {
    if (out_count == NULL || (capacity > 0 && out_nodes == NULL)) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    *out_count = 0;
    if (graph == NULL) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    *out_count = graph->order.size();
    size_t copied = graph->order.size() < capacity ? graph->order.size() : capacity;
    for (size_t i = 0; i < copied; ++i) {
        out_nodes[i] = graph->order[i];
    }
    return MIKAN_GRAPH_OK;
}

int mikan_scene_graph_propagate(mikan_scene_graph *graph,
                                const mikan_transform2d *locals,
                                mikan_transform2d *out, size_t capacity,
                                size_t *out_count) {
    if (locals == NULL || out == NULL || out_count == NULL) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    *out_count = 0;
    if (graph == NULL) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    const size_t count = graph->order.size();
    if (capacity < count) {
        return MIKAN_GRAPH_BAD_ARGUMENT;
    }
    *out_count = count;
    if (count == 0) {
        return MIKAN_GRAPH_OK;
    }

    /* Index space for the caller's arrays: entry i belongs to order[i]. */
    std::unordered_map<uint64_t, size_t> index;
    index.reserve(count);
    for (size_t i = 0; i < count; ++i) {
        index[graph->order[i]] = i;
    }

    /* Dependency-safe processing order (parents strictly before children).
     * The graph is a forest, so starting from every root and walking down the
     * children lists reaches every node exactly once. */
    std::vector<size_t> pending;
    pending.reserve(count);
    for (size_t i = 0; i < count; ++i) {
        if (parent_of(graph, graph->order[i]) == 0) {
            pending.push_back(i);
        }
    }
    std::vector<size_t> topo;
    topo.reserve(count);
    for (size_t head = 0; head < pending.size(); ++head) {
        const size_t i = pending[head];
        topo.push_back(i);
        std::unordered_map<uint64_t, std::vector<uint64_t> >::const_iterator cit =
            graph->children.find(graph->order[i]);
        if (cit == graph->children.end()) {
            continue;
        }
        for (size_t k = 0; k < cit->second.size(); ++k) {
            pending.push_back(index[cit->second[k]]);
        }
    }

    for (size_t t = 0; t < topo.size(); ++t) {
        const size_t i = topo[t];
        const uint64_t p = parent_of(graph, graph->order[i]);
        if (p == 0) {
            out[i] = locals[i];
        } else {
            mikan_transform2d_compose(&out[index[p]], &locals[i], &out[i]);
        }
    }
    return MIKAN_GRAPH_OK;
}
