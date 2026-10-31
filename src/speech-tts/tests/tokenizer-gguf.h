// Model-free metadata fixture only. The tokenizer algorithms remain in bpe.h.
#pragma once
#include <cstddef>
#include <cstdint>
#include <string>
#include <utility>
#include <vector>

struct ggml_context;
struct gguf_init_params { bool no_alloc; ggml_context ** ctx; };
struct gguf_context {
    std::vector<std::string> tokens, merges;
    std::vector<std::pair<std::string, std::uint32_t>> keys;
};
inline gguf_context tokenizer_fixture;
inline int tokenizer_gguf_opened = 0, tokenizer_gguf_freed = 0;
inline bool tokenizer_fixture_tokens_present = true, tokenizer_fixture_merges_present = true;
inline bool tokenizer_fixture_null_token = false, tokenizer_fixture_null_merge = false;
inline bool tokenizer_fixture_oversized = false;
inline gguf_context * gguf_init_from_file(const char * path, gguf_init_params) {
    if (std::string(path) != "fixture") return nullptr;
    ++tokenizer_gguf_opened;
    return new gguf_context(tokenizer_fixture);
}
inline void gguf_free(gguf_context * context) { ++tokenizer_gguf_freed; delete context; }
inline std::int64_t gguf_find_key(const gguf_context * context, const char * name) {
    if (std::string(name) == "tokenizer.ggml.tokens") return tokenizer_fixture_tokens_present ? 0 : -1;
    if (std::string(name) == "tokenizer.ggml.merges") return tokenizer_fixture_merges_present ? 1 : -1;
    for (std::size_t index = 0; index < context->keys.size(); ++index) {
        if (context->keys[index].first == name) return static_cast<std::int64_t>(index) + 2;
    }
    return -1;
}
inline std::size_t gguf_get_arr_n(const gguf_context * context, std::int64_t key) {
    if (tokenizer_fixture_oversized) return std::size_t(1) << 32;
    return (key == 0 ? context->tokens : context->merges).size();
}
inline const char * gguf_get_arr_str(const gguf_context * context, std::int64_t key, std::size_t index) {
    if ((key == 0 && tokenizer_fixture_null_token) || (key == 1 && tokenizer_fixture_null_merge)) return nullptr;
    return (key == 0 ? context->tokens : context->merges).at(index).c_str();
}
inline std::uint32_t gguf_get_val_u32(const gguf_context * context, std::int64_t key) {
    return context->keys.at(static_cast<std::size_t>(key - 2)).second;
}
