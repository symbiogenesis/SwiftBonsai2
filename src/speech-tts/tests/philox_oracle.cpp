// Model-free test driver; the unmodified pinned algorithm is in reference/philox.h.
// Only this test executable compiles that algorithm. Production calls safe Rust.
#include "philox.h"
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <io.h>

int main(int argc, char ** argv) {
    if (argc != 2 || _setmode(_fileno(stdout), _O_BINARY) == -1) {
        return 1;
    }
    FILE * input = nullptr;
    if (fopen_s(&input, argv[1], "rb") != 0 || !input) {
        return 2;
    }
    int64_t seed = 0;
    while (std::fread(&seed, sizeof(seed), 1, input) == 1) {
        int64_t subsequence = 0;
        uint32_t counter = 0;
        if (std::fread(&subsequence, sizeof(subsequence), 1, input) != 1 ||
            std::fread(&counter, sizeof(counter), 1, input) != 1) {
            std::fclose(input);
            return 3;
        }
        float value = 0.0f;
        philox_uniform_fill(seed, subsequence, counter, &value, 1);
        uint32_t bits = 0;
        static_assert(sizeof(bits) == sizeof(value), "Expected 32-bit float");
        std::memcpy(&bits, &value, sizeof(bits));
        if (std::fwrite(&bits, sizeof(bits), 1, stdout) != 1) {
            std::fclose(input);
            return 4;
        }
    }
    const bool failed = std::ferror(input) != 0;
    std::fclose(input);
    return failed || std::fflush(stdout) != 0 ? 5 : 0;
}
