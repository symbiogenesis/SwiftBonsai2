#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace swift_bitcos {
// Source layout: Prism 73bfe6cc485554822f80c473a3c0c4f25e80f41b.
using Pq2Block = std::array<std::uint8_t, 34>;
using PtqBlock = std::array<std::uint8_t, 28>;
struct Q8KBlock {
    float scale;
    std::array<std::int8_t, 256> values;
    std::array<std::int16_t, 16> sums; // Native layout; not used by these dot kernels.
};
static_assert(sizeof(Q8KBlock) == 292 && offsetof(Q8KBlock, values) == 4);
bool neon_available() noexcept;
bool dotprod_enabled() noexcept;
float half_to_float(std::uint16_t bits) noexcept;
PtqBlock pack_ptq(const Pq2Block& source);
Pq2Block unpack_ptq(const PtqBlock& source);
float dot_pq2_scalar(std::span<const Pq2Block> weights, std::span<const Q8KBlock> activations);
float dot_pq2_neon(std::span<const Pq2Block> weights, std::span<const Q8KBlock> activations);
// This comparator adapts native PTQ trit decoding to the same Q8_K inputs.
// It is deliberately not named/claimed as Prism's released PTQ x Q8_0 kernel.
float dot_ptq_neon_q8k(std::span<const PtqBlock> weights, std::span<const Q8KBlock> activations);

class Matrix final {
public:
    // A tile is one tensor row. Rows are independently accessible with a uint32
    // offset each plus a sentinel. Groups within a row stream sequentially.
    static Matrix pack(std::span<const Pq2Block> source, std::size_t rows, std::size_t blocks_per_row);
    static Matrix load(std::size_t blocks_per_row, std::vector<std::uint32_t> offsets,
                       std::vector<std::uint8_t> payload);
    std::size_t rows() const noexcept { return offsets_.empty() ? 0 : offsets_.size() - 1; }
    std::size_t blocks_per_row() const noexcept { return blocks_; }
    std::span<const std::uint8_t> payload() const noexcept { return payload_; }
    std::span<const std::uint32_t> offsets() const noexcept { return offsets_; }
    std::uint64_t serialized_bytes() const noexcept;
    std::vector<Pq2Block> unpack_row(std::size_t row) const; // Correctness only.
    float dot_scalar(std::size_t row, std::span<const Q8KBlock> activations) const;
    float dot_neon(std::size_t row, std::span<const Q8KBlock> activations) const;
private:
    Matrix(std::size_t blocks, std::vector<std::uint32_t> offsets, std::vector<std::uint8_t> payload);
    std::size_t blocks_;
    std::vector<std::uint32_t> offsets_;
    std::vector<std::uint8_t> payload_;
};
}
