//! Thin native holder and output-vector adapter; Rust owns tokenizer loading.

pub(super) const SOURCE: &str = r#"#pragma once
// Rust owns GGUF metadata intake, candidate replacement, token tables and
// encoding results. This temporary adapter serves the remaining native model.
#include "qt-error.h"
#include <cassert>
#include <climits>
#include <cstdio>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <type_traits>
#include <utility>
#include <unordered_map>
#include <vector>

extern "C" {
void bonsai_qt_bpe_free(void * owner);
bool bonsai_qt_bpe_load_gguf(void ** owner, const char * path);
int bonsai_qt_bpe_load_specials(void * owner, const char * path, const char * const * keys, int count);
bool bonsai_qt_bpe_add_special(void * owner, const uint8_t * bytes, size_t length, int32_t id);
bool bonsai_qt_bpe_encode_to(const void * owner, const uint8_t * bytes, size_t length, bool add_eos,
    void * userdata, bool (*copy)(void *, const int32_t *, size_t));
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

// Retain the original native context ownership assertions at the actual bridge.
static_assert(!std::is_copy_constructible<BPETokenizer>::value, "unique owner must not copy");
static_assert(!std::is_move_constructible<BPETokenizer>::value, "native context retains stable owner");

static inline void bpe_add_special(BPETokenizer * tok, const std::string & str, int id) {
    if (!bonsai_qt_bpe_add_special(tok->owner,
            reinterpret_cast<const uint8_t *>(str.data()), str.size(), id)) {
        qt_throw("Native speech tokenizer registration failed.");
    }
}

static bool load_bpe_from_gguf(BPETokenizer * tok, const char * path) {
    return bonsai_qt_bpe_load_gguf(&tok->owner, path);
}

static bool bpe_load_specials_from_keys(BPETokenizer * tok, const char * path,
        const char * const * keys, int count) {
    const int status = bonsai_qt_bpe_load_specials(tok->owner, path, keys, count);
    if (status < 0) { qt_throw("Native speech tokenizer registration failed."); }
    return status != 0;
}

static bool bonsai_copy_token_ids(void * userdata, const int32_t * data, size_t count) noexcept {
    // The callback cannot unwind through Rust. Rust retains and releases the
    // result on both successful copies and native allocation failure.
    try {
        auto * ids = static_cast<std::vector<int> *>(userdata);
        if (count) { ids->assign(data, data + count); }
        return true;
    } catch (...) {
        return false;
    }
}

static std::vector<int> bpe_encode(const BPETokenizer * tok, const std::string & text, bool add_eos = true) {
    std::vector<int> ids;
    if (!bonsai_qt_bpe_encode_to(tok->owner,
            reinterpret_cast<const uint8_t *>(text.data()), text.size(), add_eos,
            &ids, bonsai_copy_token_ids)) {
        qt_throw("Native speech tokenization failed.");
    }
    return ids;
}
"#;
