/* Rust-owned, dependency-free sharing policy for the pinned Prism loader.
 * Tensor creation and lifetime remain with ggml. No tensor is dereferenced here.
 */
#ifndef BONSAI_PRISM_H
#define BONSAI_PRISM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct bonsai_prism_pool bonsai_prism_pool;

typedef struct bonsai_prism_layout {
    size_t context;
    size_t buffer_type;
    bool cpu;
    bool f32;
    bool contiguous;
    int64_t dimensions[4];
    size_t strides[4];
    size_t byte_len;
} bonsai_prism_layout;

/* Pools are single-threaded and must not outlive their ggml contexts/owners.
 * Owner tokens are nonzero, live ggml_tensor pointers cast to uintptr_t.
 * Payloads are copied on remember; borrowed inputs need only survive the call.
 * Free accepts NULL. Other NULL inputs fail closed, returning false or zero.
 * Non-NULL inputs must be valid and properly aligned; bytes must contain len
 * readable bytes. Callers must provide exclusive access to a mutable pool.
 */
bonsai_prism_pool * bonsai_prism_pool_new(void);
void bonsai_prism_pool_free(bonsai_prism_pool * pool);
bool bonsai_prism_pool_eligible(const bonsai_prism_layout * layout);
uintptr_t bonsai_prism_pool_find(bonsai_prism_pool * pool,
                               const bonsai_prism_layout * layout,
                               const uint8_t * bytes, size_t len);
bool bonsai_prism_pool_remember(bonsai_prism_pool * pool,
                               const bonsai_prism_layout * layout,
                               const uint8_t * bytes, size_t len,
                               uintptr_t owner);
size_t bonsai_prism_pool_shared_tensors(const bonsai_prism_pool * pool);
size_t bonsai_prism_pool_shared_bytes(const bonsai_prism_pool * pool);
size_t bonsai_prism_pool_cached_bytes(const bonsai_prism_pool * pool);

#ifdef __cplusplus
}
#endif

#endif
