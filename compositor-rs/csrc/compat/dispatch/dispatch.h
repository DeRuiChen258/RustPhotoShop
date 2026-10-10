// Minimal stand-in for the single Grand Central Dispatch entry point the upstream macOS C
// kernels use (`dispatch_apply` in DitherPixels.c).
//
// The upstream file ships unmodified: this header only has to provide the same name and call
// shape. The bands are run in order on the calling thread, which is the same result the
// parallel version produces — every band writes to a disjoint slice of the image.
//
// Compiling a translation unit that contains blocks (`^...`) needs clang with `-fblocks` and a
// Blocks runtime at link time; `build.rs` probes for both and skips the dither kernel when they
// are missing. See BUILD.md.

#ifndef COMPOSITOR_COMPAT_DISPATCH_H
#define COMPOSITOR_COMPAT_DISPATCH_H

#include <stddef.h>

// libdispatch lets the queue be `DISPATCH_APPLY_AUTO`; the shim has nothing to choose.
#define DISPATCH_APPLY_AUTO ((void *)0)

static inline void dispatch_apply(size_t iterations, void *queue, void (^body)(size_t)) {
    (void)queue;
    for (size_t index = 0; index < iterations; ++index) {
        body(index);
    }
}

#endif
