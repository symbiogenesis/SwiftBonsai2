// Model-free byte oracle. sampling.h is the unmodified pinned header.
// The unused RNG dependency is an abort-only stub, never a native implementation.
#include "sampling.h"
#include <cstdio>
#include <fcntl.h>
#include <io.h>

int main(int argc, char ** argv) {
    // Keep the complete unmodified header, including its stochastic sampler.
    (void) &sample_top_k_p;
    if (argc != 2 || _setmode(_fileno(stdout), _O_BINARY) == -1) { return 1; }
    FILE * input = nullptr;
    if (fopen_s(&input, argv[1], "rb") != 0 || !input) { return 2; }
    uint32_t header[6];
    for (;;) {
        const auto count = std::fread(header, sizeof(uint32_t), 6, input);
        if (count == 0 && std::feof(input) && !std::ferror(input)) { break; }
        if (count != 6 || header[0] > 4096 || header[1] > 4096) { std::fclose(input); return 3; }
        const auto V = static_cast<int>(header[0]);
        const auto H = static_cast<int>(header[1]);
        float penalty;
        int32_t low, high, keep;
        std::memcpy(&penalty, &header[2], sizeof(penalty));
        std::memcpy(&low, &header[3], sizeof(low));
        std::memcpy(&high, &header[4], sizeof(high));
        std::memcpy(&keep, &header[5], sizeof(keep));
        std::vector<float> logits(header[0]);
        std::vector<int32_t> history(header[1]);
        if ((V && std::fread(logits.data(), sizeof(float), header[0], input) != header[0]) ||
            (H && std::fread(history.data(), sizeof(int32_t), header[1], input) != header[1])) {
            std::fclose(input); return 4;
        }
        apply_suppress(logits.data(), V, low, high, keep);
        if (V && std::fwrite(logits.data(), sizeof(float), header[0], stdout) != header[0]) {
            std::fclose(input); return 5;
        }
        apply_repetition_penalty(logits.data(), V, history.data(), H, penalty);
        if (V && std::fwrite(logits.data(), sizeof(float), header[0], stdout) != header[0]) {
            std::fclose(input); return 6;
        }
    }
    std::fclose(input);
    return std::fflush(stdout) != 0 ? 7 : 0;
}
