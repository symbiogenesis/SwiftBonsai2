#include "llama-lora-b-share.h"
#include "ggml-cpu.h"

#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>

static void require(bool condition, const char * message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

struct context {
    ggml_context * value;
    explicit context(size_t count) : value(ggml_init({count * ggml_tensor_overhead() + ggml_graph_overhead_custom(64, false), nullptr, true})) {
        require(value != nullptr, "context allocation failed");
    }
    ~context() { ggml_free(value); }
    context(const context &) = delete;
    context & operator=(const context &) = delete;
};

static std::vector<uint8_t> payload(size_t count, uint32_t bits = 0x3f800000U) {
    std::vector<uint8_t> result(count * sizeof(float));
    for (size_t i = 0; i < count; ++i) {
        std::memcpy(result.data() + i * sizeof(float), &bits, sizeof(bits));
    }
    return result;
}

static void test_pool_policy() {
    context ctx(256);
    context other(8);
    auto * cpu = ggml_backend_cpu_buffer_type();
    auto * source = ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 1, 5120);
    llama_lora_b_share pool;
    const auto bytes = payload(5120);
    auto * first = pool.make(ctx.value, source, cpu, bytes);
    auto * second = pool.make(ctx.value, source, cpu, bytes);
    require(first->view_src == nullptr && second->view_src == first, "equal B payload did not share");
    require(pool.make(other.value, source, cpu, bytes)->view_src == nullptr, "cross-context sharing");
    auto changed = bytes;
    changed.back() ^= 1;
    require(pool.make(ctx.value, source, cpu, changed)->view_src == nullptr, "unequal payload shared");
    auto zero = payload(5120, 0U);
    auto negative_zero = payload(5120, 0x80000000U);
    auto * z = pool.make(ctx.value, source, cpu, zero);
    auto * nz = pool.make(ctx.value, source, cpu, negative_zero);
    require(z != nz && nz->view_src == nullptr, "signed zero payloads conflated");
    require(pool.make(ctx.value, source, cpu, payload(5120, 0x7fc00001U))->view_src == nullptr, "first NaN is a view");
    require(pool.make(ctx.value, source, cpu, payload(5120, 0x7fc00002U))->view_src == nullptr, "NaN payloads conflated");
    require(pool.make(ctx.value, source, cpu, payload(5120, 0x7fc00001U))->view_src != nullptr, "same NaN payload did not share");
    require(!llama_lora_b_share::eligible(source, nullptr), "non-CPU buffer accepted");
    require(!llama_lora_b_share::eligible(ggml_new_tensor_2d(ctx.value, GGML_TYPE_F16, 1, 5120), cpu), "F16 accepted");
    require(!llama_lora_b_share::eligible(ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 2, 5120), cpu), "rank-two accepted");
    require(!llama_lora_b_share::eligible(ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 1, 20000), cpu), "oversized B accepted");
    auto * wrong_shape = ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 1, 5119);
    require(pool.make(ctx.value, wrong_shape, cpu, bytes)->view_src == nullptr, "incorrect byte count shared");
    auto * strided = ggml_view_2d(ctx.value, ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 2, 5120), 1, 5120, 8, 0);
    require(!llama_lora_b_share::eligible(strided, cpu), "noncontiguous B accepted");

    llama_lora_b_share bounded;
    auto * large = ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 1, 16384);
    for (uint32_t i = 0; i < 17; ++i) {
        require(bounded.make(ctx.value, large, cpu, payload(16384, i))->view_src == nullptr, "distinct B payload shared");
    }
    require(bounded.cached_bytes() == llama_lora_b_share::max_cache_bytes, "cache budget not enforced");
    require(bounded.make(ctx.value, large, cpu, payload(16384, 16))->view_src == nullptr, "uncached B unexpectedly shared");
    require(bounded.make(ctx.value, large, cpu, payload(16384, 0))->view_src != nullptr, "budget blocked existing match");
    llama_lora_b_share entry_bounded;
    auto * tiny = ggml_new_tensor_2d(ctx.value, GGML_TYPE_F32, 1, 1);
    for (uint32_t i = 0; i < 65; ++i) {
        require(entry_bounded.make(ctx.value, tiny, cpu, payload(1, i))->view_src == nullptr, "distinct tiny B shared");
    }
    require(entry_bounded.cached_bytes() == 64 * sizeof(float), "entry budget not enforced");
    require(entry_bounded.make(ctx.value, tiny, cpu, payload(1, 64))->view_src == nullptr, "entry cap cached an extra B");
    require(entry_bounded.make(ctx.value, tiny, cpu, payload(1, 0))->view_src != nullptr, "entry cap blocked existing match");
}

static void test_backend_storage_and_math() {
    context shared(512);
    context copied(512);
    auto * cpu = ggml_backend_cpu_buffer_type();
    auto bytes = payload(5120);
    // Include finite signed values and zeros in the realistic B vector.
    for (size_t i = 0; i < 5120; ++i) {
        const float value = static_cast<float>(static_cast<int>(i % 17) - 8) / 16.0f;
        std::memcpy(bytes.data() + i * sizeof(float), &value, sizeof(value));
    }
    llama_lora_b_share pool;
    std::vector<ggml_tensor *> aliases;
    std::vector<ggml_tensor *> originals;
    for (int i = 0; i < 129; ++i) {
        auto * original = ggml_new_tensor_2d(copied.value, GGML_TYPE_F32, 1, 5120);
        auto * alias = pool.make(shared.value, original, cpu, bytes);
        ggml_set_name(alias, ("b-" + std::to_string(i)).c_str());
        aliases.push_back(alias);
        originals.push_back(original);
    }
    auto * rank = ggml_new_tensor_2d(shared.value, GGML_TYPE_F32, 1, 2);
    auto * copy_rank = ggml_new_tensor_2d(copied.value, GGML_TYPE_F32, 1, 2);
    auto * actual = ggml_mul_mat(shared.value, aliases.back(), rank);
    auto * expected = ggml_mul_mat(copied.value, originals.back(), copy_rank);
    auto * shared_buffer = ggml_backend_alloc_ctx_tensors_from_buft(shared.value, cpu);
    auto * copied_buffer = ggml_backend_alloc_ctx_tensors_from_buft(copied.value, cpu);
    require(shared_buffer && copied_buffer, "backend allocation failed");
    require(pool.shared_tensors() == 128 && pool.shared_bytes() == 2621440, "unexpected deduplication count");
    require(ggml_backend_buffer_get_size(copied_buffer) - ggml_backend_buffer_get_size(shared_buffer) == 2621440,
            "backend did not eliminate the expected 2.5 MiB");
    for (size_t i = 0; i < aliases.size(); ++i) {
        require(aliases[i]->data == aliases.front()->data && aliases[i]->buffer == aliases.front()->buffer, "backend view is not an alias");
        require(std::string(aliases[i]->name) == "b-" + std::to_string(i), "alias lost its tensor name");
        require(aliases[i]->nb[0] == originals[i]->nb[0] && aliases[i]->nb[1] == originals[i]->nb[1], "alias layout changed");
        ggml_backend_tensor_set(originals[i], bytes.data(), 0, bytes.size());
    }
    ggml_backend_tensor_set(aliases.front(), bytes.data(), 0, bytes.size());
    const float factors[] = {0.375f, -0.625f};
    ggml_backend_tensor_set(rank, factors, 0, sizeof(factors));
    ggml_backend_tensor_set(copy_rank, factors, 0, sizeof(factors));
    auto * backend = ggml_backend_cpu_init();
    require(backend != nullptr, "CPU backend initialization failed");
    ggml_backend_cpu_set_n_threads(backend, 1);
    auto * actual_graph = ggml_new_graph_custom(shared.value, 64, false);
    auto * expected_graph = ggml_new_graph_custom(copied.value, 64, false);
    ggml_build_forward_expand(actual_graph, actual);
    ggml_build_forward_expand(expected_graph, expected);
    require(ggml_backend_graph_compute(backend, actual_graph) == GGML_STATUS_SUCCESS, "shared graph compute failed");
    require(ggml_backend_graph_compute(backend, expected_graph) == GGML_STATUS_SUCCESS, "copied graph compute failed");
    require(ggml_nbytes(actual) == ggml_nbytes(expected) && std::memcmp(actual->data, expected->data, ggml_nbytes(actual)) == 0,
            "shared B MUL_MAT output bytes changed");
    ggml_backend_free(backend);
    ggml_backend_buffer_free(copied_buffer);
    ggml_backend_buffer_free(shared_buffer);
}

int main() {
    try {
        test_pool_policy();
        test_backend_storage_and_math();
        std::cout << "CPU LoRA storage alias checks passed; backend saves 2621440 bytes; MUL_MAT is byte-identical\n";
        return EXIT_SUCCESS;
    } catch (const std::exception & error) {
        std::cerr << error.what() << '\n';
        return EXIT_FAILURE;
    }
}
