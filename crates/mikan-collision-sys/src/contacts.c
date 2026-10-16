#include "mikan_contacts.h"

#include <math.h>

static float min_f(float a, float b) { return a < b ? a : b; }

static float max_f(float a, float b) { return a > b ? a : b; }

static void separated(mikan_contact *out) {
    out->normal_x = 0.0f;
    out->normal_y = 0.0f;
    out->depth = 0.0f;
}

static int is_finite_box(const mikan_aabb *box) {
    return isfinite(box->min_x) && isfinite(box->min_y) && isfinite(box->max_x) &&
           isfinite(box->max_y);
}

static void contact_of(const mikan_aabb *a, const mikan_aabb *b,
                       mikan_contact *out) {
    /* Explicit finiteness checks make the contract independent of operand
     * order: a box with any non-finite coordinate is separated, never an
     * overlap. */
    if (!is_finite_box(a) || !is_finite_box(b)) {
        separated(out);
        return;
    }

    const float overlap_x = min_f(a->max_x, b->max_x) - max_f(a->min_x, b->min_x);
    const float overlap_y = min_f(a->max_y, b->max_y) - max_f(a->min_y, b->min_y);

    /* `!(x > 0)` rather than `x <= 0` so that a NaN that slips past the
     * finiteness checks reports separation. */
    if (!(overlap_x > 0.0f) || !(overlap_y > 0.0f)) {
        separated(out);
        return;
    }

    if (overlap_x <= overlap_y) {
        /* The center comparison decides which way `b` lies, without a
         * branch on equality: identical centers push `b` in +x, matching
         * the Rust reference bit for bit. */
        out->normal_x = (a->min_x + a->max_x <= b->min_x + b->max_x) ? 1.0f : -1.0f;
        out->normal_y = 0.0f;
        out->depth = overlap_x;
    } else {
        out->normal_x = 0.0f;
        out->normal_y = (a->min_y + a->max_y <= b->min_y + b->max_y) ? 1.0f : -1.0f;
        out->depth = overlap_y;
    }
}

void mikan_contacts(const mikan_aabb *bounds, size_t bounds_len,
                    const mikan_pair *pairs, size_t pair_count,
                    mikan_contact *out) {
    if (bounds == NULL || pairs == NULL || out == NULL) {
        return;
    }

    for (size_t k = 0; k < pair_count; ++k) {
        const mikan_pair pair = pairs[k];
        if (pair.first >= bounds_len || pair.second >= bounds_len) {
            separated(&out[k]);
            continue;
        }
        contact_of(&bounds[pair.first], &bounds[pair.second], &out[k]);
    }
}
