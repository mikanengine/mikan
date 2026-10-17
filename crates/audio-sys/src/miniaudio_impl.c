/*
 * The single translation unit that compiles the vendored miniaudio
 * implementation (version 0.11.25, see `third_party/miniaudio/README.md`).
 * It is built with warnings disabled and without warnings-as-errors: per
 * `docs/ffi.md`, the warnings-as-errors rule binds the code Mikan owns,
 * not pinned upstream sources compiled on their own terms.
 */

#include "mikan_miniaudio_config.h"

#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"
