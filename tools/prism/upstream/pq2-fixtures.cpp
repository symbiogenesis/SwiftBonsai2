// Derived from PrismML-Eng/llama.cpp tests/test-pq2-row-shapes.cpp at
// b87ca7225456b06b0975b8239c66b5aecf07e37c. See LICENSE for MIT terms.
// The upstream helpers produce the original std::normal_distribution fixtures;
// Rust consumes their bytes rather than substituting a different random stream.
#define main prism_original_main
#include "test-pq2-row-shapes.cpp"
#undef main

#include <fstream>
#include <stdexcept>

static void write_u32(std::ostream & out, uint32_t value) {
    const char bytes[] = { (char) value, (char) (value >> 8), (char) (value >> 16), (char) (value >> 24) };
    out.write(bytes, sizeof(bytes));
}

static void write_floats(std::ostream & out, const float * values, size_t count) {
    for (size_t i = 0; i < count; ++i) {
        uint32_t bits;
        memcpy(&bits, values + i, sizeof(bits));
        write_u32(out, bits);
    }
}

static void export_shape(std::ostream & out, int64_t k, int64_t m, int64_t n) {
    struct ggml_init_params params = { 64u * 1024 * 1024, nullptr, false };
    ggml_context * ctx = ggml_init(params);
    if (!ctx) {
        throw std::runtime_error("ggml_init failed");
    }
    ggml_tensor * a = ggml_new_tensor_2d(ctx, GGML_TYPE_PQ2_0, k, m);
    const auto weights = make_weights(k * m, 1234 + (uint32_t) k);
    ggml_quantize_chunk(GGML_TYPE_PQ2_0, weights.data(), a->data, 0, m, k, nullptr);
    ggml_tensor * b = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, k, n);
    const auto activations = make_weights(k * n, 99);
    memcpy(b->data, activations.data(), activations.size() * sizeof(float));
    ggml_tensor * c = ggml_mul_mat(ctx, a, b);
    ggml_cgraph * graph = ggml_new_graph(ctx);
    ggml_build_forward_expand(graph, c);
    ggml_graph_compute_with_ctx(ctx, graph, 4);
    std::vector<float> decoded(k * m);
    ggml_get_type_traits(GGML_TYPE_PQ2_0)->to_float(a->data, decoded.data(), k * m);

    write_u32(out, (uint32_t) k);
    write_u32(out, (uint32_t) m);
    write_u32(out, (uint32_t) n);
    write_u32(out, (uint32_t) ggml_nbytes(a));
    out.write((const char *) a->data, ggml_nbytes(a));
    write_floats(out, activations.data(), activations.size());
    write_floats(out, decoded.data(), decoded.size());
    write_floats(out, (const float *) c->data, m * n);
    ggml_free(ctx);
}

int main(int argc, char ** argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: prism-upstream-pq2-fixtures OUTPUT\n");
        return 2;
    }
    try {
        std::ofstream out(argv[1], std::ios::binary);
        out.exceptions(std::ios::badbit | std::ios::failbit);
        out.write("BSPQ2V1\n", 8);
        write_u32(out, 15);
        for (int64_t k : { 128, 256, 384, 512, 640 }) {
            for (int64_t n : { 1, 4, 16 }) {
                export_shape(out, k, 4, n);
            }
        }
        out.close();
    } catch (const std::exception & error) {
        fprintf(stderr, "%s\n", error.what());
        return 1;
    }
    return 0;
}
