#include "bitcos.h"
#include <bit>
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string_view>
using namespace swift_bitcos;
namespace {
bool in_ci() {
#ifdef _MSC_VER
    char value[16]{};
    std::size_t count = 0;
    return getenv_s(&count, value, sizeof(value), "GITHUB_ACTIONS") == 0 && std::string_view(value) == "true";
#else
    const char* result = std::getenv("GITHUB_ACTIONS");
    return result != nullptr && std::string_view(result) == "true";
#endif
}
volatile double checksum_sink = 0;
void run(std::size_t rows, unsigned iterations, const char* mode) {
    constexpr std::size_t blocks = 40; // Exact 5120-element rows, same Q8_K input for every layout.
    std::vector<Pq2Block> pq2(rows * blocks); std::vector<PtqBlock> ptq; ptq.reserve(pq2.size());
    std::uint32_t random = 117;
    for (auto& block : pq2) {
        block[1] = 0x3c;
        for (unsigned i = 0; i < 128; ++i) {
            random = random * 1664525U + 1013904223U;
            const unsigned sample = (random >> 16) % 10000, code = sample < 3276 ? 1U : (sample % 2 == 0 ? 0U : 2U);
            block[2 + i / 4] |= static_cast<std::uint8_t>(code << (2 * (i % 4)));
        }
        ptq.push_back(pack_ptq(block));
    }
    const auto compact = Matrix::pack(pq2, rows, blocks);
    std::vector<Q8KBlock> activations(blocks / 2);
    for (auto& block : activations) {
        block.scale = 0.03125F;
        for (unsigned i = 0; i < 256; ++i) block.values[i] = static_cast<std::int8_t>(static_cast<int>(i) - 128);
    }
    double reference = 0;
    // Untimed parity pass is mandatory before collecting any synthetic timings.
    for (std::size_t row = 0; row < rows; ++row) {
        const auto w = std::span<const Pq2Block>(pq2).subspan(row * blocks, blocks);
        const float expected = dot_pq2_neon(w, activations);
        if (std::bit_cast<std::uint32_t>(expected) != std::bit_cast<std::uint32_t>(compact.dot_neon(row, activations)) ||
            std::bit_cast<std::uint32_t>(expected) != std::bit_cast<std::uint32_t>(dot_ptq_neon_q8k(std::span<const PtqBlock>(ptq).subspan(row * blocks, blocks), activations))) throw std::runtime_error("synthetic parity failed");
        reference += expected;
    }
    // Three rotated paired samples. Hot/streaming describe allocated footprints,
    // not proven cache residency; no claims of cold-load or end-to-end inference.
    for (unsigned sample = 0; sample < 3; ++sample) for (unsigned order = 0; order < 3; ++order) {
        const unsigned kernel = (sample + order) % 3;
        double checksum = 0;
        const auto start = std::chrono::steady_clock::now();
        for (unsigned repeat = 0; repeat < iterations; ++repeat) for (std::size_t row = 0; row < rows; ++row) {
            if (kernel == 0) checksum += dot_pq2_neon(std::span<const Pq2Block>(pq2).subspan(row * blocks, blocks), activations);
            else if (kernel == 1) checksum += compact.dot_neon(row, activations);
            else checksum += dot_ptq_neon_q8k(std::span<const PtqBlock>(ptq).subspan(row * blocks, blocks), activations);
        }
        const auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now() - start).count();
        checksum_sink = checksum;
        if (checksum != reference * iterations) throw std::runtime_error("timed checksum failed");
        const char* names[] = {"pq2-neon-q8k", "bitcos-neon-q8k", "ptq-neon-adapted-q8k"};
        const std::uint64_t sizes[] = {static_cast<std::uint64_t>(pq2.size()) * 34, compact.serialized_bytes(), static_cast<std::uint64_t>(ptq.size()) * 28};
        std::cout << "{\"mode\":\"" << mode << "\",\"kernel\":\"" << names[kernel] << "\",\"sample\":" << sample
                  << ",\"rows\":" << rows << ",\"columns\":5120,\"iterations\":" << iterations << ",\"weight_bytes\":" << sizes[kernel]
                  << ",\"elapsed_ns\":" << ns << ",\"checksum\":" << checksum << ",\"dotprod\":" << (dotprod_enabled() ? "true" : "false") << "}\n";
    }
}
}
int main(int argc, char** argv) {
    // This guard is intentionally before construction, parity work or any timer.
    if (argc != 2 || std::string_view(argv[1]) != "--ci-synthetic" || !in_ci()) { std::cerr << "Benchmark execution requires GitHub Actions and --ci-synthetic.\n"; return 2; }
    try {
        if (!neon_available()) throw std::runtime_error("ARM64 NEON required for a representative comparison");
        run(8, 200, "hot-footprint"); run(32768, 2, "streaming-footprint"); return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
