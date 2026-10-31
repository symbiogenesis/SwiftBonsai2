// Compile the actual generated native header. Mock callbacks observe only its
// metadata handoff and RAII; the separate oracle retains all native algorithms.
#include "bpe.h"
#include <type_traits>

struct mock_owner {
    std::vector<std::pair<std::string, int32_t>> tokens, merges, specials;
    std::vector<int32_t> special_ids;
    bool finished = false;
};
static int created = 0, freed = 0, results_created = 0, results_freed = 0;
static int reject = 0, result_mode = 0;
static std::string encoded_text;
static bool encoded_eos = false;

extern "C" void * bonsai_qt_bpe_new() {
    if (reject == 1) return nullptr;
    ++created;
    return new mock_owner;
}
extern "C" void bonsai_qt_bpe_free(void * owner) {
    if (owner) { ++freed; delete static_cast<mock_owner *>(owner); }
}
extern "C" bool bonsai_qt_bpe_add_token(void * owner, const uint8_t * bytes, size_t length, int32_t id) {
    if (reject == 2) return false;
    static_cast<mock_owner *>(owner)->tokens.emplace_back(std::string(reinterpret_cast<const char *>(bytes), length), id);
    return true;
}
extern "C" bool bonsai_qt_bpe_add_merge(void * owner, const uint8_t * bytes, size_t length, int32_t rank) {
    if (reject == 3) return false;
    static_cast<mock_owner *>(owner)->merges.emplace_back(std::string(reinterpret_cast<const char *>(bytes), length), rank);
    return true;
}
extern "C" bool bonsai_qt_bpe_finish(void * owner) {
    if (reject == 4) return false;
    static_cast<mock_owner *>(owner)->finished = true;
    return true;
}
extern "C" bool bonsai_qt_bpe_add_special(void * owner, const uint8_t * bytes, size_t length, int32_t id) {
    if (reject == 5) return false;
    static_cast<mock_owner *>(owner)->specials.emplace_back(std::string(reinterpret_cast<const char *>(bytes), length), id);
    return true;
}
extern "C" bool bonsai_qt_bpe_add_special_id(void * owner, int32_t id) {
    if (reject == 6) return false;
    static_cast<mock_owner *>(owner)->special_ids.push_back(id);
    return true;
}
extern "C" void * bonsai_qt_bpe_encode(const void * owner, const uint8_t * bytes, size_t length, bool eos) {
    assert(static_cast<const mock_owner *>(owner)->finished);
    encoded_text.assign(reinterpret_cast<const char *>(bytes), length);
    encoded_eos = eos;
    if (result_mode == 1) return nullptr;
    ++results_created;
    return new std::vector<int32_t>(result_mode == 2 ? std::vector<int32_t>{} : std::vector<int32_t>{17, 18});
}
extern "C" const int32_t * bonsai_qt_bpe_result_data(const void * result) {
    return result_mode == 3 ? nullptr : static_cast<const std::vector<int32_t> *>(result)->data();
}
extern "C" size_t bonsai_qt_bpe_result_len(const void * result) {
    return static_cast<const std::vector<int32_t> *>(result)->size();
}
extern "C" void bonsai_qt_bpe_result_free(void * result) {
    if (result) { ++results_freed; delete static_cast<std::vector<int32_t> *>(result); }
}

int main() {
    static_assert(!std::is_copy_constructible<BPETokenizer>::value, "unique owner must not copy");
    static_assert(!std::is_move_constructible<BPETokenizer>::value, "native context retains stable owner");
    tokenizer_fixture.tokens = {"a", "a", "<eos>"};
    tokenizer_fixture.merges = {"a a", "a a"};
    tokenizer_fixture.keys = {{"first", 1}, {"invalid", UINT32_MAX}};
    {
        BPETokenizer tokenizer;
        assert(!load_bpe_from_gguf(&tokenizer, "missing"));
        assert(!tokenizer.owner && created == 0 && freed == 0);
        assert(load_bpe_from_gguf(&tokenizer, "fixture"));
        auto * owner = static_cast<mock_owner *>(tokenizer.owner);
        assert(owner->finished && owner->tokens.size() == 3 && owner->merges.size() == 2);
        assert(owner->tokens[0] == std::make_pair(std::string("a"), int32_t(0)));
        assert(owner->tokens[1] == std::make_pair(std::string("a"), int32_t(1)));
        assert(owner->merges[1] == std::make_pair(std::string("a a"), int32_t(1)));
        assert(tokenizer_gguf_opened == tokenizer_gguf_freed);
        const char * keys[] = {"missing", "first", "invalid", "first"};
        assert(bpe_load_specials_from_keys(&tokenizer, "fixture", keys, 4));
        assert((owner->special_ids == std::vector<int32_t>{1, -1, 1}));
        bpe_add_special(&tokenizer, "<voice>", 2);
        assert(owner->specials[0] == std::make_pair(std::string("<voice>"), int32_t(2)));
        for (reject = 1; reject <= 4; ++reject) {
            const int before_created = created, before_freed = freed;
            assert(!load_bpe_from_gguf(&tokenizer, "fixture"));
            assert(tokenizer.owner == owner);
            assert(created - before_created == (reject == 1 ? 0 : 1));
            assert(freed - before_freed == (reject == 1 ? 0 : 1));
            assert(tokenizer_gguf_opened == tokenizer_gguf_freed);
        }
        reject = 0;
        for (bool * flag : {&tokenizer_fixture_tokens_present, &tokenizer_fixture_merges_present}) {
            *flag = false;
            assert(!load_bpe_from_gguf(&tokenizer, "fixture"));
            assert(tokenizer.owner == owner);
            *flag = true;
        }
        for (bool * flag : {&tokenizer_fixture_null_token, &tokenizer_fixture_null_merge, &tokenizer_fixture_oversized}) {
            *flag = true;
            assert(!load_bpe_from_gguf(&tokenizer, "fixture"));
            assert(tokenizer.owner == owner);
            *flag = false;
        }
        assert(tokenizer_gguf_opened == tokenizer_gguf_freed);
        reject = 5;
        bool threw = false;
        try { bpe_add_special(&tokenizer, "bad", 0); } catch (const std::runtime_error &) { threw = true; }
        assert(threw);
        reject = 6;
        threw = false;
        try { bpe_load_specials_from_keys(&tokenizer, "fixture", keys, 4); } catch (const std::runtime_error &) { threw = true; }
        assert(threw && tokenizer_gguf_opened == tokenizer_gguf_freed);
        reject = 0;
        const std::string text("a\0b", 3);
        const auto ids = bpe_encode(&tokenizer, text, false);
        assert((ids == std::vector<int>{17, 18}));
        assert(encoded_text == text && !encoded_eos && results_created == results_freed);
        result_mode = 2;
        assert(bpe_encode(&tokenizer, "", true).empty());
        assert(encoded_text.empty() && encoded_eos && results_created == results_freed);
        for (result_mode = 1; result_mode <= 3; result_mode += 2) {
            threw = false;
            try { (void) bpe_encode(&tokenizer, "failure"); } catch (const std::runtime_error &) { threw = true; }
            assert(threw && results_created == results_freed);
        }
        result_mode = 0;
        const int before_freed = freed;
        assert(load_bpe_from_gguf(&tokenizer, "fixture"));
        assert(tokenizer.owner != owner && freed == before_freed + 1);
        assert((bpe_encode(&tokenizer, "recovery") == std::vector<int>{17, 18}));
    }
    assert(created == freed && results_created == results_freed);
    assert(tokenizer_gguf_opened == tokenizer_gguf_freed);
    return 0;
}
