#include "mikan_transform.h"

#include <stddef.h>

void mikan_transform2d_compose(const mikan_transform2d *parent,
                               const mikan_transform2d *local,
                               mikan_transform2d *out) {
    mikan_transform2d result;

    if (parent == NULL || local == NULL || out == NULL) {
        return;
    }

    /* out = parent ∘ local; the grouping of every product and sum is fixed
     * and mirrored exactly by the Rust reference, so both sides round
     * identically. `out` may alias `parent` or `local`: every input is read
     * before the first write. */
    result.translation_x =
        parent->translation_x +
        (parent->cos_rotation * (parent->scale_x * local->translation_x) -
         parent->sin_rotation * (parent->scale_y * local->translation_y));
    result.translation_y =
        parent->translation_y +
        (parent->sin_rotation * (parent->scale_x * local->translation_x) +
         parent->cos_rotation * (parent->scale_y * local->translation_y));
    result.rotation = parent->rotation + local->rotation;
    result.scale_x = parent->scale_x * local->scale_x;
    result.scale_y = parent->scale_y * local->scale_y;
    result.cos_rotation = parent->cos_rotation * local->cos_rotation -
                          parent->sin_rotation * local->sin_rotation;
    result.sin_rotation = parent->sin_rotation * local->cos_rotation +
                          parent->cos_rotation * local->sin_rotation;

    *out = result;
}
