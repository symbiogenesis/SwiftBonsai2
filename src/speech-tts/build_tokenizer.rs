//! Native object access and RAII only; Rust owns every tokenizer table and rule.

pub(super) const SOURCE: &str = r#"#pragma once
// The GGUF reader remains native. Token tables, special registration and BPE
// encoding live in one Rust owner per qt_context, freed after worker teardown.
#include "gguf.h"
#include "qt-error.h"
#include <cassert>
#include <climits>
#include <cstdio>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <utility>
#include <unordered_map>
#include <vector>

extern "C" {
void * bonsai_qt_bpe_new();
void bonsai_qt_bpe_free(void * owner);
bool bonsai_qt_bpe_add_token(void * owner, const uint8_t * bytes, size_t length, int32_t id);
bool bonsai_qt_bpe_add_merge(void * owner, const uint8_t * bytes, size_t length, int32_t rank);
bool bonsai_qt_bpe_finish(void * owner);
bool bonsai_qt_bpe_add_special(void * owner, const uint8_t * bytes, size_t length, int32_t id);
bool bonsai_qt_bpe_add_special_id(void * owner, int32_t id);
void * bonsai_qt_bpe_encode(const void * owner, const uint8_t * bytes, size_t length, bool add_eos);
const int32_t * bonsai_qt_bpe_result_data(const void * result);
size_t bonsai_qt_bpe_result_len(const void * result);
void bonsai_qt_bpe_result_free(void * result);
}

struct BPETokenizer {
    void * owner = nullptr;
    BPETokenizer() = default;
    BPETokenizer(const BPETokenizer &) = delete;
    BPETokenizer & operator=(const BPETokenizer &) = delete;
    BPETokenizer(BPETokenizer &&) = delete;
    BPETokenizer & operator=(BPETokenizer &&) = delete;
    ~BPETokenizer() { bonsai_qt_bpe_free(owner); }
};

struct BonsaiBpeGGUF {
    gguf_context * value;
    ~BonsaiBpeGGUF() { if (value) { gguf_free(value); } }
};

static inline void bpe_add_special(BPETokenizer * tok, const std::string & str, int id) {
    if (!bonsai_qt_bpe_add_special(tok->owner,
            reinterpret_cast<const uint8_t *>(str.data()), str.size(), id)) {
        qt_throw("Native speech tokenizer registration failed.");
    }
}

static bool load_bpe_from_gguf(BPETokenizer * tok, const char * gguf_path) {
    gguf_init_params params = { true, nullptr };
    BonsaiBpeGGUF context{gguf_init_from_file(gguf_path, params)};
    if (!context.value) { return false; }
    const auto token_key = gguf_find_key(context.value, "tokenizer.ggml.tokens");
    const auto merge_key = gguf_find_key(context.value, "tokenizer.ggml.merges");
    if (token_key < 0 || merge_key < 0) { return false; }
    const auto n_tokens = gguf_get_arr_n(context.value, token_key);
    const auto n_merges = gguf_get_arr_n(context.value, merge_key);
    if (n_tokens > static_cast<uint64_t>(std::numeric_limits<int32_t>::max()) ||
        n_merges > static_cast<uint64_t>(std::numeric_limits<int32_t>::max())) { return false; }
    BPETokenizer replacement;
    replacement.owner = bonsai_qt_bpe_new();
    if (!replacement.owner) { return false; }
    for (uint64_t i = 0; i < n_tokens; ++i) {
        const char * text = gguf_get_arr_str(context.value, token_key, static_cast<size_t>(i));
        if (!text || !bonsai_qt_bpe_add_token(replacement.owner,
                reinterpret_cast<const uint8_t *>(text), std::strlen(text), static_cast<int32_t>(i))) { return false; }
    }
    for (uint64_t i = 0; i < n_merges; ++i) {
        const char * text = gguf_get_arr_str(context.value, merge_key, static_cast<size_t>(i));
        if (!text || !bonsai_qt_bpe_add_merge(replacement.owner,
                reinterpret_cast<const uint8_t *>(text), std::strlen(text), static_cast<int32_t>(i))) { return false; }
    }
    if (!bonsai_qt_bpe_finish(replacement.owner)) { return false; }
    std::swap(tok->owner, replacement.owner);
    return true;
}

static bool bpe_load_specials_from_keys(BPETokenizer * tok, const char * gguf_path,
        const char * const * keys, int n_keys) {
    gguf_init_params params = { true, nullptr };
    BonsaiBpeGGUF context{gguf_init_from_file(gguf_path, params)};
    if (!context.value) { return false; }
    for (int i = 0; i < n_keys; ++i) {
        const auto key = gguf_find_key(context.value, keys[i]);
        if (key < 0) { continue; }
        if (!bonsai_qt_bpe_add_special_id(tok->owner,
                static_cast<int32_t>(gguf_get_val_u32(context.value, key)))) {
            qt_throw("Native speech tokenizer registration failed.");
        }
    }
    return true;
}

static std::vector<int> bpe_encode(const BPETokenizer * tok, const std::string & text, bool add_eos = true) {
    struct result_owner {
        void * value;
        ~result_owner() { bonsai_qt_bpe_result_free(value); }
    } result{bonsai_qt_bpe_encode(tok->owner,
            reinterpret_cast<const uint8_t *>(text.data()), text.size(), add_eos)};
    if (!result.value) { qt_throw("Native speech tokenization failed."); }
    const size_t count = bonsai_qt_bpe_result_len(result.value);
    std::vector<int> ids;
    if (count) {
        const int32_t * data = bonsai_qt_bpe_result_data(result.value);
        if (!data) { qt_throw("Native speech tokenization failed."); }
        ids.assign(data, data + count);
    }
    return ids;
}
"#;
