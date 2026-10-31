// The complete unmodified pinned bpe.h supplies every tokenizer algorithm.
// This executable only reads fixture metadata and serializes observations.
#include "bpe.h"
#include <fcntl.h>
#include <io.h>
#include <limits>
#include <stdexcept>

static std::uint32_t read_u32(FILE * input) {
    std::uint32_t value;
    if (std::fread(&value, sizeof(value), 1, input) != 1) throw std::runtime_error("truncated fixture");
    return value;
}
static std::uint32_t count(FILE * input, std::uint32_t maximum) {
    const auto value = read_u32(input);
    if (value > maximum) throw std::runtime_error("fixture extent");
    return value;
}
static std::string text(FILE * input) {
    const auto size = count(input, 65536);
    std::string value(size, '\0');
    if (size && std::fread(value.data(), 1, size, input) != size) throw std::runtime_error("truncated text");
    return value;
}
static void write_u32(std::uint32_t value) {
    if (std::fwrite(&value, sizeof(value), 1, stdout) != 1) throw std::runtime_error("output");
}
static void write_i32(std::int32_t value) {
    std::uint32_t bits;
    std::memcpy(&bits, &value, sizeof(bits));
    write_u32(bits);
}
static void write_text(const std::string & value) {
    write_u32(static_cast<std::uint32_t>(value.size()));
    if (!value.empty() && std::fwrite(value.data(), 1, value.size(), stdout) != value.size()) {
        throw std::runtime_error("output text");
    }
}

int main(int argc, char ** argv) {
    if (argc != 2 || _setmode(_fileno(stdout), _O_BINARY) == -1) return 1;
    FILE * input = nullptr;
    if (fopen_s(&input, argv[1], "rb") != 0 || !input) return 2;
    try {
        const auto models = count(input, 32);
        for (std::uint32_t model = 0; model < models; ++model) {
            tokenizer_fixture = {};
            auto size = count(input, 4096);
            for (std::uint32_t index = 0; index < size; ++index) tokenizer_fixture.tokens.push_back(text(input));
            size = count(input, 4096);
            for (std::uint32_t index = 0; index < size; ++index) tokenizer_fixture.merges.push_back(text(input));
            size = count(input, 64);
            for (std::uint32_t index = 0; index < size; ++index) {
                auto key = text(input);
                const auto id = read_u32(input);
                tokenizer_fixture.keys.emplace_back(std::move(key), id);
            }
            BPETokenizer tokenizer;
            if (!load_bpe_from_gguf(&tokenizer, "fixture")) throw std::runtime_error("load");
            std::vector<std::string> key_storage;
            size = count(input, 64);
            for (std::uint32_t index = 0; index < size; ++index) key_storage.push_back(text(input));
            std::vector<const char *> keys;
            for (const auto & key : key_storage) keys.push_back(key.c_str());
            if (!bpe_load_specials_from_keys(&tokenizer, "fixture", keys.data(), static_cast<int>(keys.size()))) {
                throw std::runtime_error("special load");
            }
            size = count(input, 64);
            for (std::uint32_t index = 0; index < size; ++index) {
                auto spelling = text(input);
                const auto id = read_u32(input);
                bpe_add_special(&tokenizer, spelling, static_cast<int>(id));
            }
            write_i32(tokenizer.n_vocab);
            write_i32(tokenizer.eos_id);
            write_u32(static_cast<std::uint32_t>(tokenizer.id_to_str.size()));
            for (const auto & spelling : tokenizer.id_to_str) write_text(spelling);
            write_u32(static_cast<std::uint32_t>(tokenizer.specials.size()));
            for (const auto & special : tokenizer.specials) {
                write_text(special.first);
                write_i32(special.second);
            }
            const auto requests = count(input, 8192);
            for (std::uint32_t request = 0; request < requests; ++request) {
                const auto body = text(input);
                const auto eos = count(input, 1);
                const auto chunks = gpt2_pre_tokenize(body);
                write_u32(static_cast<std::uint32_t>(chunks.size()));
                for (const auto & chunk : chunks) write_text(chunk);
                const auto ids = bpe_encode(&tokenizer, body, eos != 0);
                write_u32(static_cast<std::uint32_t>(ids.size()));
                for (const auto id : ids) write_i32(id);
            }
        }
        if (std::fgetc(input) != EOF || std::ferror(input)) throw std::runtime_error("fixture tail");
        std::fclose(input);
        return std::fflush(stdout) != 0 ? 3 : 0;
    } catch (const std::exception & error) {
        std::fprintf(stderr, "tokenizer oracle: %s\n", error.what());
        std::fclose(input);
        return 4;
    }
}
