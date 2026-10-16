/*
 * Mikan collision narrow phase — the engine's one native entry point.
 *
 * This header is the contract; the Rust wrapper mirrors it in
 * crates/mikan-collision-sys/src/lib.rs and the reference implementation in
 * crates/mikan-collision/src/reference.rs, and the test suite cross-checks the
 * three against each other.
 *
 * Semantics of mikan_contacts:
 *
 *   - It is a pure function: it reads `bounds` and `pairs`, writes one contact
 *     per pair into `out`, and retains no pointer. There is no allocation, no
 *     global state, and no error condition to translate.
 *   - For pair k, `out[k]` describes `bounds[pairs[k].first]` (`a`) and
 *     `bounds[pairs[k].second]` (`b`). When the boxes overlap, `depth > 0` and
 *     `normal` is a unit axis vector pointing from `a` toward `b` — the
 *     direction that separates `b` from `a`. Otherwise the normal is zero and
 *     `depth` is zero. Boxes that merely touch are *separated*: a resting
 *     contact has depth 0, so resolution cannot jitter.
 *   - Inputs are validated defensively: null pointers produce no output, an
 *     out-of-range pair index produces a separated contact, and a box with
 *     any non-finite coordinate (NaN or infinity) is separated, never an
 *     overlap — the check is explicit, so the result does not depend on
 *     operand order.
 *   - Thread safety: the function is reentrant and touches only its arguments.
 *     Distinct calls may run concurrently; a single call must not alias `out`
 *     with `bounds` or `pairs`.
 */
#ifndef MIKAN_CONTACTS_H
#define MIKAN_CONTACTS_H

#include <stddef.h>

typedef struct mikan_aabb {
    float min_x;
    float min_y;
    float max_x;
    float max_y;
} mikan_aabb;

typedef struct mikan_pair {
    unsigned int first;
    unsigned int second;
} mikan_pair;

typedef struct mikan_contact {
    float normal_x;
    float normal_y;
    float depth;
} mikan_contact;

/* `out` must have room for exactly `pair_count` contacts. */
void mikan_contacts(const mikan_aabb *bounds, size_t bounds_len,
                    const mikan_pair *pairs, size_t pair_count,
                    mikan_contact *out);

#endif /* MIKAN_CONTACTS_H */
