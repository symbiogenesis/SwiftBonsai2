// Repository-owned overlay for pinned Prism 73bfe6cc. No arithmetic changes.
#pragma once

#include "ggml.h"
#include "ggml-backend.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <vector>

// LoRA weights are immutable after loading. Only ordinary CPU, contiguous F32,
// rank-one B vectors are eligible. A same-context view preserves a tensor's own
// name and layout while the backend allocator allocates the owner just once.
// Byte comparisons deliberately distinguish signed zero and NaN payloads.
class llama_lora_b_share {
public:
    static constexpr size_t max_tensor_bytes = 64 * 1024;
    static constexpr size_t max_cache_bytes = 1024 * 1024;
    static constexpr size_t max_entries = 64;

    static bool eligible(const ggml_tensor * tensor, ggml_backend_buffer_type_t buft) {
        return buft == ggml_backend_cpu_buffer_type() &&
            tensor->type == GGML_TYPE_F32 && tensor->ne[0] == 1 &&
            tensor->ne[1] > 0 && tensor->ne[2] == 1 && tensor->ne[3] == 1 &&
            ggml_is_contiguous(tensor) && ggml_nbytes(tensor) <= max_tensor_bytes;
    }

    ggml_tensor * make(ggml_context * ctx, const ggml_tensor * source,
                       ggml_backend_buffer_type_t buft, const std::vector<uint8_t> & bytes) {
        if (!eligible(source, buft) || bytes.size() != ggml_nbytes(source)) {
            return ggml_dup_tensor(ctx, source);
        }
        for (const auto & entry : entries_) {
            if (entry.ctx == ctx && entry.buft == buft && same_layout(source, entry.owner) &&
                entry.bytes == bytes) {
                ++shared_tensors_;
                shared_bytes_ += bytes.size();
                return ggml_view_tensor(ctx, entry.owner);
            }
        }
        ggml_tensor * result = ggml_dup_tensor(ctx, source);
        if (entries_.size() < max_entries && bytes.size() <= max_cache_bytes - cached_bytes_) {
            entries_.push_back({ctx, buft, result, bytes});
            cached_bytes_ += bytes.size();
        }
        return result;
    }

    size_t shared_tensors() const { return shared_tensors_; }
    size_t shared_bytes() const { return shared_bytes_; }
    size_t cached_bytes() const { return cached_bytes_; }

private:
    static bool same_layout(const ggml_tensor * a, const ggml_tensor * b) {
        return a->type == b->type &&
            std::equal(a->ne, a->ne + GGML_MAX_DIMS, b->ne) &&
            std::equal(a->nb, a->nb + GGML_MAX_DIMS, b->nb);
    }
    struct entry {
        ggml_context * ctx;
        ggml_backend_buffer_type_t buft;
        ggml_tensor * owner;
        std::vector<uint8_t> bytes;
    };
    std::vector<entry> entries_;
    size_t cached_bytes_ = 0;
    size_t shared_tensors_ = 0;
    size_t shared_bytes_ = 0;
};
