#include "bitcos.h"
#include <algorithm>
#include <bit>
#include <limits>
#include <stdexcept>
#include <utility>
#if defined(_M_ARM64) || defined(__aarch64__)
#include <arm_neon.h>
#define SWIFT_NEON 1
#else
#define SWIFT_NEON 0
#endif

namespace swift_bitcos {
namespace {
[[noreturn]] void bad(const char* reason) { throw std::invalid_argument(reason); }
std::uint16_t scale_bits(const std::uint8_t* p) noexcept {
    return static_cast<std::uint16_t>(p[0] | (static_cast<unsigned>(p[1]) << 8));
}
void check_scale(const std::uint8_t* p) {
    const auto s = scale_bits(p);
    if ((s & 0x7fff) == 0 || (s & 0x7c00) == 0x7c00) bad("finite nonzero FP16 scale required");
}
unsigned code(const Pq2Block& b, std::size_t i) noexcept { return (b[2 + i / 4] >> (2 * (i % 4))) & 3U; }
void check_pq2(const Pq2Block& b) {
    check_scale(b.data());
    for (std::size_t i = 0; i < 128; ++i) if (code(b, i) == 3) bad("PQ2 +2 symbol is not ternary");
}
void check_dot(std::size_t blocks, std::size_t activation_blocks) {
    if (blocks == 0 || blocks % 2 != 0 || activation_blocks != blocks / 2) bad("Q8_K dot requires nonempty pairs of PQ2 groups");
}
unsigned present(const std::uint8_t* block) noexcept {
#if SWIFT_NEON
    return vaddvq_u8(vcntq_u8(vld1q_u8(block + 2)));
#else
    unsigned n = 0;
    for (unsigned i = 2; i < 18; ++i) n += std::popcount(static_cast<unsigned>(block[i]));
    return n;
#endif
}
std::size_t block_bytes(const std::uint8_t* block) noexcept { return 18 + (present(block) + 7U) / 8U; }
constexpr auto make_lut() {
    std::array<std::uint32_t, 256> result{};
    for (unsigned mask = 0; mask < 16; ++mask) {
        for (unsigned signs = 0; signs < 16; ++signs) {
            unsigned rank = 0;
            for (unsigned i = 0; i < 4; ++i) {
                unsigned value = 0;
                if ((mask & (1U << i)) != 0) value = (signs & (1U << rank++)) != 0 ? 1U : 255U;
                result[mask * 16 + signs] |= value << (i * 8);
            }
        }
    }
    return result;
}
constexpr auto lut = make_lut();
constexpr auto make_counts() {
    std::array<unsigned, 16> counts{};
    for (unsigned i = 0; i < 16; ++i) for (unsigned b = 0; b < 4; ++b) counts[i] += (i >> b) & 1U;
    return counts;
}
constexpr auto nibble_counts = make_counts();
// At most two guarded byte loads; no assumption of readable sentinel/padding.
unsigned sign_window(const std::uint8_t* signs, unsigned bytes, unsigned cursor) noexcept {
    const unsigned index = cursor / 8, shift = cursor % 8;
    unsigned word = index < bytes ? signs[index] : 0;
    if (shift > 4 && index + 1 < bytes) word |= static_cast<unsigned>(signs[index + 1]) << 8;
    return (word >> shift) & 15U;
}
float scale_product(const std::uint8_t* block, const Q8KBlock& activation) noexcept {
    return half_to_float(scale_bits(block)) * activation.scale;
}
#if SWIFT_NEON
int32x4_t dot16(int32x4_t sum, int8x16_t w, int8x16_t a) noexcept {
#if BITCOS_DOTPROD
    return vdotq_s32(sum, w, a);
#else
    const auto low = vmull_s8(vget_low_s8(w), vget_low_s8(a));
    const auto high = vmull_s8(vget_high_s8(w), vget_high_s8(a));
    return vaddq_s32(sum, vaddq_s32(vpaddlq_s16(low), vpaddlq_s16(high)));
#endif
}
int32x4_t bitcos_dot_block(const std::uint8_t* block, const std::int8_t* activation, unsigned sign_bytes) noexcept {
    auto sum = vdupq_n_s32(0);
    unsigned cursor = 0;
    for (unsigned group = 0; group < 8; ++group) {
        std::uint32_t words[4];
        for (unsigned nibble = 0; nibble < 4; ++nibble) {
            const unsigned position = group * 16 + nibble * 4;
            const unsigned mask = (block[2 + position / 8] >> (position % 8)) & 15U;
            const unsigned signs = sign_window(block + 18, sign_bytes, cursor);
            words[nibble] = lut[mask * 16 + signs];
            cursor += nibble_counts[mask];
        }
        // Materialize only sixteen live int8 lanes; no expanded tensor or PQ2 copy.
        const auto low = vcreate_u8(static_cast<std::uint64_t>(words[0]) | (static_cast<std::uint64_t>(words[1]) << 32));
        const auto high = vcreate_u8(static_cast<std::uint64_t>(words[2]) | (static_cast<std::uint64_t>(words[3]) << 32));
        sum = dot16(sum, vreinterpretq_s8_u8(vcombine_u8(low, high)), vld1q_s8(activation + group * 16));
    }
    return sum;
}
int pq2_dot_block(const Pq2Block& block, const std::int8_t* activation) noexcept {
    // Same byte replication/shifts as pinned Prism ARM PQ2 x Q8_K, nrc=1.
    const std::uint8_t lo[16] = {0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3};
    const std::uint8_t hi[16] = {4,4,4,4,5,5,5,5,6,6,6,6,7,7,7,7};
    const std::int8_t shifts[16] = {0,-2,-4,-6,0,-2,-4,-6,0,-2,-4,-6,0,-2,-4,-6};
    auto sum = vdupq_n_s32(0);
    for (unsigned k = 0; k < 4; ++k) {
        const auto raw8 = vld1_u8(block.data() + 2 + k * 8);
        const auto raw = vcombine_u8(raw8, raw8);
        const auto decode = [&](const std::uint8_t* indices) {
            return vsubq_s8(vreinterpretq_s8_u8(vandq_u8(vshlq_u8(vqtbl1q_u8(raw, vld1q_u8(indices)), vld1q_s8(shifts)), vdupq_n_u8(3))), vdupq_n_s8(1));
        };
        sum = dot16(sum, decode(lo), vld1q_s8(activation + 32 * k));
        sum = dot16(sum, decode(hi), vld1q_s8(activation + 32 * k + 16));
    }
    return vaddvq_s32(sum);
}
int8x16_t trits(uint8x16_t bytes) noexcept {
    return vsubq_s8(vreinterpretq_s8_u8(vshrq_n_u8(vhaddq_u8(bytes, vshrq_n_u8(bytes, 1)), 6)), vdupq_n_s8(1));
}
int ptq_dot_block(const PtqBlock& block, const std::int8_t* activation) noexcept {
    auto sum = vdupq_n_s32(0);
    auto first = vld1q_u8(block.data());
    for (unsigned n = 0; n < 5; ++n) {
        sum = dot16(sum, trits(first), vld1q_s8(activation + n * 16));
        first = vmulq_u8(first, vdupq_n_u8(3));
    }
    auto tail = vld1_u8(block.data() + 16);
    for (unsigned n = 0; n < 5; ++n) {
        const auto weights = trits(vcombine_u8(tail, vdup_n_u8(0)));
        const auto values = vcombine_s8(vld1_s8(activation + 80 + n * 8), vdup_n_s8(0));
        sum = dot16(sum, weights, values);
        tail = vmul_u8(tail, vdup_n_u8(3));
    }
    int total = vaddvq_s32(sum);
    constexpr unsigned pow3[4] = {1,3,9,27};
    for (unsigned n = 0; n < 4; ++n) for (unsigned h = 0; h < 2; ++h) {
        const auto q = static_cast<std::uint8_t>(block[24 + h] * pow3[n]);
        total += (static_cast<int>((static_cast<unsigned>(q) * 3) >> 8) - 1) * activation[120 + n * 2 + h];
    }
    return total;
}
#endif
}
bool neon_available() noexcept { return SWIFT_NEON != 0; }
bool dotprod_enabled() noexcept { return SWIFT_NEON != 0 && BITCOS_DOTPROD != 0; }
float half_to_float(std::uint16_t value) noexcept {
    const std::uint32_t sign = static_cast<std::uint32_t>(value & 0x8000) << 16;
    std::uint32_t mantissa = value & 1023U, exponent = (value >> 10) & 31U;
    if (exponent == 0) {
        if (mantissa == 0) return std::bit_cast<float>(sign);
        exponent = 113;
        while ((mantissa & 1024U) == 0) { mantissa <<= 1; --exponent; }
        mantissa &= 1023U;
    } else if (exponent == 31) exponent = 255;
    else exponent += 112;
    return std::bit_cast<float>(sign | (exponent << 23) | (mantissa << 13));
}
PtqBlock pack_ptq(const Pq2Block& source) {
    check_pq2(source);
    PtqBlock result{};
    unsigned position = 0;
    for (const unsigned width : {16U, 8U}) {
        const unsigned start = position / 5;
        for (unsigned m = 0; m < width; ++m) {
            unsigned q = 0;
            for (unsigned n = 0; n < 5; ++n) q = q * 3 + code(source, position + m + n * width);
            result[start + m] = static_cast<std::uint8_t>((q * 256 + 242) / 243);
        }
        position += width * 5;
    }
    for (unsigned h = 0; h < 2; ++h) {
        unsigned q = 0;
        for (unsigned n = 0; n < 4; ++n) q = q * 3 + code(source, 120 + h + n * 2);
        result[24 + h] = static_cast<std::uint8_t>((q * 3 * 256 + 242) / 243);
    }
    result[26] = source[0]; result[27] = source[1];
    return result;
}
Pq2Block unpack_ptq(const PtqBlock& source) {
    check_scale(source.data() + 26);
    Pq2Block result{}; result[0] = source[26]; result[1] = source[27];
    constexpr unsigned powers[5] = {1,3,9,27,81};
    unsigned position = 0;
    for (const unsigned width : {16U,8U}) {
        const unsigned start = position / 5;
        for (unsigned n = 0; n < 5; ++n) for (unsigned m = 0; m < width; ++m) {
            const auto q = static_cast<std::uint8_t>(source[start + m] * powers[n]);
            const unsigned c = (static_cast<unsigned>(q) * 3) >> 8;
            const unsigned i = position + n * width + m;
            result[2 + i / 4] |= static_cast<std::uint8_t>(c << (2 * (i % 4)));
        }
        position += width * 5;
    }
    for (unsigned n = 0; n < 4; ++n) for (unsigned h = 0; h < 2; ++h) {
        const auto q = static_cast<std::uint8_t>(source[24 + h] * powers[n]);
        const unsigned c = (static_cast<unsigned>(q) * 3) >> 8, i = 120 + n * 2 + h;
        result[2 + i / 4] |= static_cast<std::uint8_t>(c << (2 * (i % 4)));
    }
    return result;
}
float dot_pq2_scalar(std::span<const Pq2Block> weights, std::span<const Q8KBlock> activations) {
    check_dot(weights.size(), activations.size()); float total = 0;
    for (std::size_t b = 0; b < weights.size(); ++b) {
        int sum = 0;
        for (unsigned i = 0; i < 128; ++i) sum += (static_cast<int>(code(weights[b], i)) - 1) * activations[b / 2].values[(b % 2) * 128 + i];
        total += scale_product(weights[b].data(), activations[b / 2]) * static_cast<float>(sum);
    }
    return total;
}
float dot_pq2_neon(std::span<const Pq2Block> weights, std::span<const Q8KBlock> activations) {
    check_dot(weights.size(), activations.size());
#if SWIFT_NEON
    float total = 0;
    for (std::size_t b = 0; b < weights.size(); ++b) total += scale_product(weights[b].data(), activations[b / 2]) * static_cast<float>(pq2_dot_block(weights[b], activations[b / 2].values.data() + (b % 2) * 128));
    return total;
#else
    return dot_pq2_scalar(weights, activations);
#endif
}
float dot_ptq_neon_q8k(std::span<const PtqBlock> weights, std::span<const Q8KBlock> activations) {
    check_dot(weights.size(), activations.size()); float total = 0;
    for (std::size_t b = 0; b < weights.size(); ++b) {
#if SWIFT_NEON
        const int sum = ptq_dot_block(weights[b], activations[b / 2].values.data() + (b % 2) * 128);
#else
        const auto decoded = unpack_ptq(weights[b]); int sum = 0;
        for (unsigned i = 0; i < 128; ++i) sum += (static_cast<int>(code(decoded, i)) - 1) * activations[b / 2].values[(b % 2) * 128 + i];
#endif
        total += scale_product(weights[b].data() + 26, activations[b / 2]) * static_cast<float>(sum);
    }
    return total;
}
Matrix::Matrix(std::size_t blocks, std::vector<std::uint32_t> offsets, std::vector<std::uint8_t> payload)
    : blocks_(blocks), offsets_(std::move(offsets)), payload_(std::move(payload)) {}
Matrix Matrix::pack(std::span<const Pq2Block> source, std::size_t rows, std::size_t blocks) {
    if (rows == 0 || blocks == 0 || blocks % 2 != 0 || rows > source.size() / blocks || rows * blocks != source.size()) bad("invalid matrix dimensions");
    if (source.size() > std::numeric_limits<std::uint32_t>::max() / 34U) bad("uint32 row index capacity exceeded");
    std::size_t bytes = 0;
    for (const auto& block : source) {
        check_pq2(block); unsigned nonzero = 0;
        for (unsigned i = 0; i < 128; ++i) nonzero += code(block, i) != 1 ? 1U : 0U;
        bytes += 18 + (nonzero + 7U) / 8U;
    }
    // Exact-size construction: never reserve a persistent PQ2-sized allocation.
    std::vector<std::uint8_t> payload(bytes);
    std::vector<std::uint32_t> offsets(rows + 1);
    std::size_t cursor = 0;
    for (std::size_t row = 0; row < rows; ++row) {
        offsets[row] = static_cast<std::uint32_t>(cursor);
        for (std::size_t b = 0; b < blocks; ++b) {
            const auto& source_block = source[row * blocks + b]; check_pq2(source_block);
            std::array<std::uint8_t, 34> block{}; block[0] = source_block[0]; block[1] = source_block[1];
            unsigned rank = 0;
            for (unsigned i = 0; i < 128; ++i) {
                const unsigned c = code(source_block, i); if (c == 1) continue;
                block[2 + i / 8] |= static_cast<std::uint8_t>(1U << (i % 8));
                if (c == 2) block[18 + rank / 8] |= static_cast<std::uint8_t>(1U << (rank % 8));
                ++rank;
            }
            const std::size_t count = 18 + (rank + 7) / 8;
            std::copy_n(block.begin(), count, payload.begin() + static_cast<std::ptrdiff_t>(cursor));
            cursor += count;
        }
    }
    offsets[rows] = static_cast<std::uint32_t>(cursor);
    return Matrix(blocks, std::move(offsets), std::move(payload));
}
Matrix Matrix::load(std::size_t blocks, std::vector<std::uint32_t> offsets, std::vector<std::uint8_t> payload) {
    if (blocks == 0 || blocks % 2 != 0 || offsets.size() < 2 || offsets.front() != 0 || offsets.back() != payload.size()) bad("invalid compact matrix index");
    for (std::size_t row = 0; row + 1 < offsets.size(); ++row) {
        std::size_t cursor = offsets[row], end = offsets[row + 1];
        if (end > payload.size() || cursor > end || blocks > (end - cursor) / 18) bad("invalid compact row extent");
        for (std::size_t b = 0; b < blocks; ++b) {
            if (end - cursor < 18) bad("truncated compact block prefix");
            const auto* block = payload.data() + cursor; check_scale(block);
            const auto nonzero = present(block); const std::size_t bytes = 18 + (nonzero + 7U) / 8U;
            if (bytes > end - cursor) bad("truncated compact signs");
            if (nonzero % 8 != 0 && (block[bytes - 1] >> (nonzero % 8)) != 0) bad("noncanonical compact sign padding");
            cursor += bytes;
        }
        if (cursor != end) bad("extra compact row bytes");
    }
    return Matrix(blocks, std::move(offsets), std::move(payload));
}
std::uint64_t Matrix::serialized_bytes() const noexcept { return static_cast<std::uint64_t>(payload_.size()) + static_cast<std::uint64_t>(offsets_.size()) * 4; }
std::vector<Pq2Block> Matrix::unpack_row(std::size_t row) const {
    if (row >= rows()) bad("row out of bounds");
    auto cursor = static_cast<std::size_t>(offsets_[row]); std::vector<Pq2Block> result(blocks_);
    for (auto& block : result) {
        const auto* src = payload_.data() + cursor; block[0] = src[0]; block[1] = src[1]; unsigned rank = 0;
        for (unsigned i = 0; i < 128; ++i) {
            unsigned c = 1;
            if ((src[2 + i / 8] & (1U << (i % 8))) != 0) { c = ((src[18 + rank / 8] >> (rank % 8)) & 1U) * 2; ++rank; }
            block[2 + i / 4] |= static_cast<std::uint8_t>(c << (2 * (i % 4)));
        }
        cursor += block_bytes(src);
    }
    return result;
}
float Matrix::dot_scalar(std::size_t row, std::span<const Q8KBlock> a) const {
    if (row >= rows()) bad("row out of bounds"); check_dot(blocks_, a.size());
    std::size_t cursor = offsets_[row]; float total = 0;
    for (std::size_t b = 0; b < blocks_; ++b) {
        const auto* block = payload_.data() + cursor; int sum = 0; unsigned rank = 0;
        for (unsigned i = 0; i < 128; ++i) if ((block[2 + i / 8] & (1U << (i % 8))) != 0) {
            const int w = ((block[18 + rank / 8] >> (rank % 8)) & 1U) != 0 ? 1 : -1; ++rank;
            sum += w * a[b / 2].values[(b % 2) * 128 + i];
        }
        total += scale_product(block, a[b / 2]) * static_cast<float>(sum); cursor += block_bytes(block);
    }
    return total;
}
float Matrix::dot_neon(std::size_t row, std::span<const Q8KBlock> a) const {
    if (row >= rows()) bad("row out of bounds"); check_dot(blocks_, a.size());
#if SWIFT_NEON
    std::size_t cursor = offsets_[row]; float total = 0;
    for (std::size_t b = 0; b < blocks_; ++b) {
        const auto* block = payload_.data() + cursor;
        const unsigned sign_bytes = (present(block) + 7U) / 8U;
        const int sum = vaddvq_s32(bitcos_dot_block(block, a[b / 2].values.data() + (b % 2) * 128, sign_bytes));
        total += scale_product(block, a[b / 2]) * static_cast<float>(sum); cursor += 18 + sign_bytes;
    }
    return total;
#else
    return dot_scalar(row, a);
#endif
}
}
