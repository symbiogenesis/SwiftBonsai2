#include "bitcos.h"
#include <algorithm>
#include <bit>
#include <iostream>
#include <stdexcept>
#include <string_view>
#include <utility>
using namespace swift_bitcos;
namespace {
void require(bool ok, const char* text) { if (!ok) throw std::runtime_error(text); }
template<class F> void rejects(F&& action) {
    bool failed = false;
    try { action(); } catch (const std::invalid_argument&) { failed = true; }
    require(failed, "malformed input accepted");
}
void set_code(Pq2Block& b, unsigned i, unsigned c) {
    const unsigned shift = 2 * (i % 4);
    b[2 + i / 4] = static_cast<std::uint8_t>((b[2 + i / 4] & ~(3U << shift)) | (c << shift));
}
Pq2Block zeros(std::uint16_t scale = 0x3c00) {
    Pq2Block b{}; b.fill(0x55); b[0] = static_cast<std::uint8_t>(scale); b[1] = static_cast<std::uint8_t>(scale >> 8); return b;
}
void exact(float a, float b) { require(std::bit_cast<std::uint32_t>(a) == std::bit_cast<std::uint32_t>(b), "dot product bits differ"); }
std::vector<Q8KBlock> activations(std::size_t count) {
    std::vector<Q8KBlock> result(count);
    for (std::size_t b = 0; b < count; ++b) {
        result[b].scale = (b % 2 != 0 ? -1.0F : 1.0F) * 0.03125F;
        for (unsigned i = 0; i < 256; ++i) result[b].values[i] = static_cast<std::int8_t>(static_cast<int>((i * 37 + b * 19) % 256) - 128);
    }
    return result;
}
void codec() {
    exact(half_to_float(0x3c00), 1.0F); exact(half_to_float(0x0001), 0x1p-24F);
    exact(half_to_float(0xbc00), -1.0F); exact(half_to_float(0x8000), -0.0F);
    for (unsigned position = 0; position < 128; ++position) for (unsigned c = 0; c < 3; ++c) {
        std::array<Pq2Block, 2> input{zeros(), zeros()}; set_code(input[0], position, c);
        const auto packed = Matrix::pack(input, 1, 2);
        require(packed.unpack_row(0) == std::vector<Pq2Block>(input.begin(), input.end()), "BITCOS symbol order differs");
        const auto payload = packed.payload();
        require(payload[2 + position / 8] == (c == 1 ? 0U : 1U << (position % 8)), "presence bit differs");
        if (c != 1) require(payload[18] == (c == 2 ? 1 : 0), "sign meaning differs");
        require(unpack_ptq(pack_ptq(input[0])) == input[0], "PTQ symbol order differs");
    }
    for (unsigned nonzero = 0; nonzero <= 128; ++nonzero) {
        std::array<Pq2Block, 2> input{zeros(0x83ff), zeros(0x7bff)};
        for (unsigned i = 0; i < nonzero; ++i) set_code(input[0], (i * 37) % 128, i % 2 == 0 ? 0 : 2);
        const auto packed = Matrix::pack(input, 1, 2);
        require(packed.payload().size() == 36 + (nonzero + 7) / 8, "block sign byte rounding differs");
        require(packed.serialized_bytes() == packed.payload().size() + 8, "row index accounting differs");
        require(packed.unpack_row(0) == std::vector<Pq2Block>(input.begin(), input.end()), "BITCOS sign boundary differs");
        require(unpack_ptq(pack_ptq(input[0])) == input[0], "PTQ sign boundary differs");
    }
}
void dot() {
    for (const std::size_t blocks : {2U,40U,48U,136U}) {
        std::vector<Pq2Block> input(3 * blocks); std::uint32_t random = 0x12345678;
        constexpr std::uint16_t scales[] = {0x0001,0x03ff,0x0400,0x3555,0xbc00,0x83ff,0x7bff};
        for (std::size_t b = 0; b < input.size(); ++b) {
            input[b] = zeros(scales[b % 7]);
            for (unsigned i = 0; i < 128; ++i) { random = random * 1664525U + 1013904223U; set_code(input[b], i, (random >> 16) % 3); }
        }
        const auto packed = Matrix::pack(input, 3, blocks); const auto a = activations(blocks / 2);
        for (std::size_t row = 0; row < 3; ++row) {
            const auto w = std::span<const Pq2Block>(input).subspan(row * blocks, blocks);
            std::vector<PtqBlock> ptq; for (const auto& b : w) ptq.push_back(pack_ptq(b));
            const float reference = dot_pq2_scalar(w, a);
            exact(reference, dot_pq2_neon(w, a)); exact(reference, packed.dot_scalar(row, a));
            exact(reference, packed.dot_neon(row, a)); exact(reference, dot_ptq_neon_q8k(ptq, a));
        }
    }
    const auto a = activations(1);
    for (unsigned i = 0; i < 256; ++i) for (const unsigned c : {0U, 2U}) {
        std::array<Pq2Block, 2> input{zeros(), zeros()}; set_code(input[i / 128], i % 128, c);
        const auto packed = Matrix::pack(input, 1, 2);
        exact(dot_pq2_scalar(input, a), packed.dot_neon(0, a));
    }
}
void matrix() {
    const std::array<Pq2Block, 2> input{zeros(), zeros()};
    const auto packed = Matrix::pack(input, 1, 2);
    const auto reload = Matrix::load(2, std::vector<std::uint32_t>(packed.offsets().begin(), packed.offsets().end()), std::vector<std::uint8_t>(packed.payload().begin(), packed.payload().end()));
    require(reload.unpack_row(0) == packed.unpack_row(0), "validated reload differs");
    rejects([&] { Matrix::pack(input, 0, 2); }); rejects([&] { Matrix::pack(input, 1, 1); });
    rejects([&] { Matrix::pack(input, 2, 2); }); rejects([&] { Matrix::load(2, {0,35}, std::vector<std::uint8_t>(35)); });
    rejects([&] { Matrix::load(2, {1,36}, std::vector<std::uint8_t>(36)); });
    for (const unsigned scale : {0U,0x8000U,0x7c00U,0xfc00U,0x7e01U}) {
        std::array<Pq2Block, 2> bad_input{zeros(static_cast<std::uint16_t>(scale)), zeros()};
        rejects([&] { Matrix::pack(bad_input, 1, 2); }); rejects([&] { pack_ptq(bad_input[0]); });
    }
    for (unsigned i = 0; i < 128; ++i) {
        auto bad_input = input; set_code(bad_input[0], i, 3);
        rejects([&] { Matrix::pack(bad_input, 1, 2); }); rejects([&] { pack_ptq(bad_input[0]); });
    }
    auto one = input; set_code(one[0], 0, 2); const auto single = Matrix::pack(one, 1, 2);
    auto payload = std::vector<std::uint8_t>(single.payload().begin(), single.payload().end());
    auto offsets = std::vector<std::uint32_t>(single.offsets().begin(), single.offsets().end());
    payload[18] |= 128;
    rejects([&] { Matrix::load(2, offsets, payload); });
    payload[18] = 1; payload.pop_back(); --offsets.back();
    rejects([&] { Matrix::load(2, offsets, payload); });
    rejects([&] { packed.dot_neon(1, activations(1)); }); rejects([&] { packed.dot_scalar(0, activations(2)); });
    rejects([&] { dot_pq2_neon(input, activations(0)); });
}
}
int main(int argc, char** argv) {
    try {
        const std::string_view selected = argc == 2 ? argv[1] : "all";
        if (argc > 2 || (selected != "all" && selected != "codec" && selected != "dot" && selected != "matrix")) throw std::invalid_argument("use bitcos-tests [codec|dot|matrix|all]");
        if (selected == "all" || selected == "codec") codec();
        if (selected == "all" || selected == "dot") dot();
        if (selected == "all" || selected == "matrix") matrix();
        std::cout << "PASS " << selected << " neon=" << neon_available() << " dotprod=" << dotprod_enabled() << '\n'; return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
