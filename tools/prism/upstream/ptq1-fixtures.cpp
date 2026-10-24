// Derived from PrismML-Eng/llama.cpp tests/test-ptq1_0-element-map.cpp at
// b87ca7225456b06b0975b8239c66b5aecf07e37c. See LICENSE for MIT terms.
#define main prism_original_main
#include "test-ptq1_0-element-map.cpp"
#undef main

#include <fstream>
#include <exception>

int main(int argc, char ** argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: prism-upstream-ptq1-fixtures OUTPUT\n");
        return 2;
    }
    try {
        std::ofstream out(argv[1], std::ios::binary);
        out.exceptions(std::ios::badbit | std::ios::failbit);
        out.write("BSPTQV1\n", 8);
        unsigned seed = 12345;
        for (int trial = 0; trial < 20000; ++trial) {
            block_ptq1_0 block = {};
            for (int i = 0; i < 24; ++i) {
                seed = seed * 1103515245u + 12345u;
                block.qs[i] = (seed >> 16) & 0xFF;
            }
            for (int i = 0; i < 2; ++i) {
                seed = seed * 1103515245u + 12345u;
                block.qh[i] = (seed >> 16) & 0xFF;
            }
            // Scale was unused and uninitialized in the upstream integer test.
            // A unit scale permits comparison with our production float decoder.
            block.d = 0x3c00;
            out.write((const char *) block.qs, sizeof(block.qs));
            out.write((const char *) block.qh, sizeof(block.qh));
            out.put(0x00);
            out.put(0x3c);
            int reference[QK_PTQ1_0];
            cpu_ref(&block, reference);
            for (int e = 0; e < QK_PTQ1_0; ++e) {
                out.put((char) (reference[e] + 1));
            }
            for (int e = 0; e < QK_PTQ1_0; ++e) {
                out.put((char) (ptq1_0_trit(&block, e) + 1));
            }
        }
        out.close();
    } catch (const std::exception & error) {
        fprintf(stderr, "%s\n", error.what());
        return 1;
    }
    return 0;
}
