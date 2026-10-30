// Fixed two-token correctness capture against the released 73bfe6cc DLL ABI.
// Build with that exact source's llama.h/ggml headers. This is a diagnostic,
// never a serving engine. The caller must verify and hold the exact runtime,
// current-weight PTQ1 model, approved adapter and this executable throughout.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <llama.h>
#include <ggml-backend.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

static constexpr const wchar_t * commit = L"73bfe6cc485554822f80c473a3c0c4f25e80f41b";
static constexpr size_t max_state = 512ULL * 1024 * 1024;
static constexpr int vocabulary = 248320;

static void require(bool condition, const char * message) {
    if (!condition) throw std::runtime_error(message);
}

static bool absolute(const std::wstring & path) {
    return path.size() > 3 && path[1] == L':' && (path[2] == L'\\' || path[2] == L'/')
        && path.find(L'%') == std::wstring::npos && path.find(L"..") == std::wstring::npos;
}

static std::wstring basename(const std::wstring & path) {
    return path.substr(path.find_last_of(L"\\/") + 1);
}

static std::string utf8(const std::wstring & text) {
    const int length = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS,
        text.c_str(), -1, nullptr, 0, nullptr, nullptr);
    require(length > 0 && length <= 32768, "invalid or oversized UTF-16 path");
    std::vector<char> bytes(static_cast<size_t>(length));
    require(WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text.c_str(), -1,
        bytes.data(), length, nullptr, nullptr) == length, "UTF-8 conversion failed");
    return std::string(bytes.data(), bytes.size() - 1);
}

struct library {
    HMODULE handle;
    explicit library(const std::wstring & path) : handle(LoadLibraryExW(path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32)) {
        require(handle != nullptr, "pinned DLL initialization failed");
    }
    ~library() { FreeLibrary(handle); }
    library(const library &) = delete;
    library & operator=(const library &) = delete;
};

template<class T> static T symbol(const library & dll, const char * name) {
    const FARPROC address = GetProcAddress(dll.handle, name);
    require(address != nullptr, name);
    static_assert(sizeof(T) == sizeof(address));
    T result;
    std::memcpy(&result, &address, sizeof(result));
    return result;
}

static void write_new(const std::wstring & path, const void * data, size_t size) {
    require(size <= max_state, "snapshot exceeds fixed byte bound");
    const HANDLE file = CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ,
        nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    require(file != INVALID_HANDLE_VALUE, "snapshot already exists or cannot be created");
    const auto * bytes = static_cast<const unsigned char *>(data);
    bool success = true;
    for (size_t at = 0; at < size;) {
        const DWORD count = static_cast<DWORD>((size - at) < 1048576 ? size - at : 1048576);
        DWORD written = 0;
        if (!WriteFile(file, bytes + at, count, &written, nullptr) || written != count) {
            success = false;
            break;
        }
        at += written;
    }
    const BOOL closed = CloseHandle(file);
    require(success && closed, "snapshot write failed; partial evidence retained");
}

static std::string token_record(size_t position, llama_token token, size_t state_bytes,
    const float * logits) {
    int greedy = 0;
    for (int i = 0; i < vocabulary; ++i) {
        require(std::isfinite(logits[i]), "nonfinite native logit");
        if (logits[i] > logits[greedy]) greedy = i;
    }
    return "{\"position\":" + std::to_string(position) + ",\"inputToken\":"
        + std::to_string(token) + ",\"greedyToken\":" + std::to_string(greedy)
        + ",\"logitsCount\":248320,\"sequenceStateBytes\":" + std::to_string(state_bytes) + "}";
}

static int run(int argc, wchar_t ** argv) {
    require(argc == 3 || argc == 7, "usage: --abi-probe RUNTIME | capture RUNTIME PTQ1 EVIDENCE none|ADAPTER 0|1.3");
    const bool probe = std::wstring(argv[1]) == L"--abi-probe";
    require((probe && argc == 3) || (!probe && argc == 7 && std::wstring(argv[1]) == L"capture"), "invalid capture mode");
    const std::wstring runtime = argv[2];
    require(absolute(runtime) && basename(runtime) == commit, "exact absolute pinned runtime directory required");
    library base(runtime + L"\\ggml-base.dll");
    library ggml(runtime + L"\\ggml.dll");
    library cpu(runtime + L"\\ggml-cpu.dll");
    library llama(runtime + L"\\llama.dll");
#define FN(name) auto name = symbol<decltype(&::name)>(llama, #name)
    FN(llama_model_default_params);
    FN(llama_context_default_params);
    const auto defaults_model = llama_model_default_params();
    const auto defaults_context = llama_context_default_params();
    require(defaults_model.dspark_head_source == nullptr && defaults_context.cb_eval == nullptr,
        "native default parameters differ from expected source ABI");
    if (probe) {
        std::printf("{\"status\":\"native-default-parameter-abi-probe-passed\",\"modelParamsBytes\":%zu,"
            "\"contextParamsBytes\":%zu,\"batchBytes\":%zu,\"modelLoaded\":false}\n",
            sizeof(llama_model_params), sizeof(llama_context_params), sizeof(llama_batch));
        return 0;
    }
    const std::wstring model_path = argv[3], evidence = argv[4], adapter_path = argv[5], scale_text = argv[6];
    require(absolute(model_path) && basename(model_path) == L"Swift-Bonsai-2-current-repacked-PTQ1_0.gguf",
        "exact current-weight PTQ1 filename and absolute path required");
    require(absolute(evidence) && GetFileAttributesW(evidence.c_str()) != INVALID_FILE_ATTRIBUTES,
        "existing absolute evidence directory required");
    const bool with_adapter = adapter_path != L"none";
    require(!with_adapter || (absolute(adapter_path) && basename(adapter_path) == L"bonsai-abliterate-lora.gguf"),
        "approved absolute adapter path required");
    require(scale_text == L"0" || (with_adapter && scale_text == L"1.3"), "only fixed zero and 1.3 arms are supported");
    float scale = scale_text == L"1.3" ? 1.3F : 0.0F;
    FN(llama_backend_init);
    FN(llama_backend_free);
    FN(llama_model_load_from_file);
    FN(llama_model_free);
    FN(llama_init_from_model);
    FN(llama_free);
    FN(llama_model_get_vocab);
    FN(llama_vocab_n_tokens);
    FN(llama_adapter_lora_init);
    FN(llama_adapter_lora_free);
    FN(llama_set_adapters_lora);
    FN(llama_batch_get_one);
    FN(llama_decode);
    FN(llama_get_logits_ith);
    FN(llama_state_seq_get_size);
    FN(llama_state_seq_get_data);
#undef FN
    auto load_backend = symbol<decltype(&::ggml_backend_load)>(ggml, "ggml_backend_load");
    require(load_backend(utf8(runtime + L"\\ggml-cpu.dll").c_str()) != nullptr, "exact CPU backend registration failed");
    llama_backend_init();
    struct backend_guard {
        decltype(llama_backend_free) release;
        ~backend_guard() { release(); }
    } backend{llama_backend_free};
    auto model_params = defaults_model;
    model_params.n_gpu_layers = 0;
    model_params.load_mode = LLAMA_LOAD_MODE_MMAP;
    model_params.use_extra_bufts = false;
    model_params.load_mtp = false;
    std::unique_ptr<llama_model, decltype(llama_model_free)> model(
        llama_model_load_from_file(utf8(model_path).c_str(), model_params), llama_model_free);
    require(model != nullptr, "native model loading failed");
    require(llama_vocab_n_tokens(llama_model_get_vocab(model.get())) == vocabulary, "native vocabulary mismatch");
    std::unique_ptr<llama_adapter_lora, decltype(llama_adapter_lora_free)> adapter(
        with_adapter ? llama_adapter_lora_init(model.get(), utf8(adapter_path).c_str()) : nullptr,
        llama_adapter_lora_free);
    require(!with_adapter || adapter != nullptr, "approved native adapter loading failed");
    auto context_params = defaults_context;
    context_params.n_ctx = 8192;
    context_params.n_batch = 128;
    context_params.n_ubatch = 64;
    context_params.n_seq_max = 1;
    context_params.n_rs_seq = 0;
    context_params.n_threads = 4;
    context_params.n_threads_batch = 4;
    context_params.type_k = GGML_TYPE_F16;
    context_params.type_v = GGML_TYPE_F16;
    context_params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    context_params.offload_kqv = false;
    context_params.op_offload = false;
    context_params.no_perf = true;
    std::unique_ptr<llama_context, decltype(llama_free)> context(
        llama_init_from_model(model.get(), context_params), llama_free);
    require(context != nullptr, "native context allocation failed");
    if (with_adapter) {
        llama_adapter_lora * pointer = adapter.get();
        require(llama_set_adapters_lora(context.get(), &pointer, 1, &scale) == 0, "native adapter application failed");
    }
    const llama_token supplied[2] = {248045, 872};
    std::string records;
    for (size_t step = 0; step < 2; ++step) {
        llama_token token = supplied[step];
        require(llama_decode(context.get(), llama_batch_get_one(&token, 1)) == 0, "native decode failed");
        const float * logits = llama_get_logits_ith(context.get(), -1);
        require(logits != nullptr, "native logits missing");
        const size_t size = llama_state_seq_get_size(context.get(), 0);
        require(size > 0 && size <= max_state, "native sequence-state byte count exceeds bound");
        std::vector<uint8_t> state(size);
        require(llama_state_seq_get_data(context.get(), state.data(), state.size(), 0) == size, "native sequence-state serialization truncated");
        const std::string record = token_record(step, token, size, logits);
        const std::wstring suffix = std::to_wstring(step);
        write_new(evidence + L"\\logits-" + suffix + L".f32le", logits, vocabulary * sizeof(float));
        write_new(evidence + L"\\state-" + suffix + L".native-seq", state.data(), state.size());
        if (step) records += ',';
        records += record;
        std::printf("%s\n", record.c_str());
        std::fflush(stdout);
    }
    const std::string report = "{\"schemaVersion\":1,\"nativeCommit\":\"73bfe6cc485554822f80c473a3c0c4f25e80f41b\","
        "\"modelSha256\":\"e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5\","
        "\"adapterLoaded\":" + std::string(with_adapter ? "true" : "false") + ",\"adapterScale\":"
        + (scale_text == L"1.3" ? "1.3" : "0") + ",\"contextTokens\":8192,\"threads\":4,"
        "\"stateFormat\":\"pinned-llama_state_seq_get_data-sequence-zero\",\"writerBasisQualified\":false,"
        "\"numericalParityQualified\":false,\"steps\":[" + records + "]}\n";
    write_new(evidence + L"\\native-capture.json", report.data(), report.size());
    return 0;
}

int wmain(int argc, wchar_t ** argv) {
    try { return run(argc, argv); }
    catch (const std::exception & error) {
        std::fprintf(stderr, "Native correctness capture failed: %s\n", error.what());
        return 1;
    }
}
