//! Exact-source overlays that transfer native decode-workspace ownership to Rust.

use std::io;

use sha2::{Digest as _, Sha256};

fn identity(text: &str, bytes: usize, digest: [u8; 32]) -> io::Result<String> {
    let normalized = text.replace("\r\n", "\n");
    if normalized.len() != bytes
        || <[u8; 32]>::from(Sha256::digest(normalized.as_bytes())) != digest
    {
        return Err(io::Error::other(
            "Pinned Qwen graph ownership source differs.",
        ));
    }
    Ok(normalized)
}

fn replace(text: &str, before: &str, after: &str) -> io::Result<String> {
    if text.matches(before).count() != 1 {
        return Err(io::Error::other(
            "Pinned Qwen graph ownership anchor differs.",
        ));
    }
    Ok(text.replacen(before, after, 1))
}

const GRAPH_TYPE_BYTES: usize = 2116;
const GRAPH_TYPE_SHA256: [u8; 32] = [
    0xe6, 0xf6, 0xdd, 0xee, 0x6c, 0x3d, 0x81, 0x07, 0x43, 0x4b, 0x8a, 0x63, 0x92, 0x63, 0x8f, 0xb7,
    0x5f, 0xfd, 0xd1, 0xec, 0x7f, 0xd1, 0x85, 0x9a, 0xed, 0xa7, 0x50, 0x80, 0x33, 0xd5, 0x18, 0xa1,
];

const FORWARD_BYTES: usize = 40377;
const FORWARD_SHA256: [u8; 32] = [
    0xe4, 0xd3, 0x56, 0x7d, 0x0c, 0xc1, 0x32, 0x46, 0xb3, 0xcd, 0xea, 0x58, 0x49, 0x1c, 0x5e, 0x35,
    0xa0, 0x56, 0x9e, 0x30, 0xc0, 0x79, 0x6c, 0x9e, 0x09, 0x50, 0x45, 0x5a, 0xd6, 0xc7, 0xc2, 0x2a,
];

const PIPELINE_HEADER_BYTES: usize = 10145;
const PIPELINE_HEADER_SHA256: [u8; 32] = [
    0xb8, 0xd1, 0xdd, 0xed, 0x24, 0x38, 0x0f, 0xcc, 0xc3, 0x00, 0x45, 0xc6, 0xfe, 0xbd, 0x08, 0x75,
    0xba, 0xb7, 0xda, 0x2b, 0xb0, 0x71, 0xa0, 0xc7, 0xbb, 0xc8, 0x17, 0x9d, 0x7d, 0xb5, 0xaa, 0x43,
];

const PIPELINE_BYTES: usize = 66658;
const PIPELINE_SHA256: [u8; 32] = [
    0xa8, 0x97, 0xcc, 0xa5, 0x16, 0x5b, 0xc9, 0x01, 0xa6, 0x79, 0x81, 0x9f, 0xd9, 0x59, 0x4a, 0xa2,
    0x61, 0x7a, 0x82, 0x43, 0xcb, 0x77, 0x71, 0x57, 0x2d, 0x88, 0xb8, 0xe2, 0x5e, 0xb1, 0xfc, 0x85,
];

const NATIVE_HOLDER: &str = r#"
// Runner owns the graph cache and every completed graph through Rust Drop.
#include "ggml-backend.h"
#include <cstddef>
#include <type_traits>
extern "C" {
void * bonsai_qt_decode_cache_new(size_t windows);
void bonsai_qt_decode_cache_free(void * cache);
void * bonsai_qt_decode_cache_begin(void * cache, int tokens, int width, void * userdata,
    void * (*build)(void *), void (*destroy)(void *));
void bonsai_qt_decode_cache_complete(void * cache);
}
struct BonsaiDecodeCache {
    void * value = nullptr;
    BonsaiDecodeCache() = default;
    BonsaiDecodeCache(const BonsaiDecodeCache &) = delete;
    BonsaiDecodeCache & operator=(const BonsaiDecodeCache &) = delete;
    BonsaiDecodeCache(BonsaiDecodeCache &&) = delete;
    BonsaiDecodeCache & operator=(BonsaiDecodeCache &&) = delete;
    ~BonsaiDecodeCache() { clear(); }
    bool init(size_t windows) {
        clear();
        value = bonsai_qt_decode_cache_new(windows);
        return value != nullptr;
    }
    void clear() noexcept {
        if (value) { bonsai_qt_decode_cache_free(value); value = nullptr; }
    }
};
static_assert(!std::is_copy_constructible<BonsaiDecodeCache>::value, "Rust graph cache has one native holder");
static_assert(!std::is_move_constructible<BonsaiDecodeCache>::value, "Rust graph cache handle stays stable");
struct BonsaiDecodeLease {
    void * cache;
    ggml_backend_t backend;
    BonsaiDecodeLease(void * owner, ggml_backend_t device) : cache(owner), backend(device) {}
    BonsaiDecodeLease(const BonsaiDecodeLease &) = delete;
    BonsaiDecodeLease & operator=(const BonsaiDecodeLease &) = delete;
    ~BonsaiDecodeLease() noexcept {
        // This also covers native exceptions and early returns. Synchronize
        // before Rust may release graph-local allocations on the next request.
        ggml_backend_synchronize(backend);
        bonsai_qt_decode_cache_complete(cache);
    }
};
"#;

const BUILD_CALLBACK: &str = r"// No C++ exception may cross the Rust cache's callback boundary.
struct BonsaiDecodeBuildArgs {
    const TalkerWeights * tw;
    KVCache * kv;
    ggml_backend_t backend;
    struct ggml_tensor * hidden_bridge;
    struct ggml_tensor * const * acoustic_embd;
    int n_acoustic;
    int n_kv_pad;
    int width;
    bool use_flash_attn;
    bool clamp_fp16;
};
static void bonsai_decode_graph_destroy(void * raw) noexcept {
    auto * graph = static_cast<TalkerDecodeGraph *>(raw);
    talker_decode_graph_free(graph);
    delete graph;
}
static void * bonsai_decode_graph_build(void * raw) noexcept {
    const auto * args = static_cast<const BonsaiDecodeBuildArgs *>(raw);
    auto * graph = new (std::nothrow) TalkerDecodeGraph;
    if (!graph) { return nullptr; }
    try {
        if (talker_decode_graph_build(args->tw, args->kv, args->backend,
                args->hidden_bridge, args->acoustic_embd, args->n_acoustic,
                args->n_kv_pad, args->width, args->use_flash_attn, args->clamp_fp16, graph)) {
            return graph;
        }
    } catch (...) {
        // The Rust owner has not published this incomplete allocation.
    }
    bonsai_decode_graph_destroy(graph);
    return nullptr;
}

";

const OLD_SELECTION: &str = r"    TalkerDecodeGraph * tg = &graphs[(size_t) ((n_kv_pad + 255) / 256 - 1)];
    if (tg->ctx && tg->N != N) {
        talker_decode_graph_free(tg);
    }
    if (!tg->ctx && !talker_decode_graph_build(tw, kv, backend, hidden_bridge, acoustic_embd, n_acoustic, n_kv_pad, N,
                                               use_flash_attn, clamp_fp16, tg)) {
        return false;
    }
";

const RUST_SELECTION: &str = r"    BonsaiDecodeBuildArgs build{tw, kv, backend, hidden_bridge, acoustic_embd,
        n_acoustic, n_kv_pad, N, use_flash_attn, clamp_fp16};
    auto * tg = static_cast<TalkerDecodeGraph *>(bonsai_qt_decode_cache_begin(
        graphs.value, n_kv_pad, N, &build, bonsai_decode_graph_build, bonsai_decode_graph_destroy));
    if (!tg) { return false; }
    BonsaiDecodeLease lease(graphs.value, backend);
";

const OLD_FIELD: &str = r"    std::vector<TalkerDecodeGraph> talker_decode_graphs;  // one per 256 step window class, lazy";

const RUST_FIELD: &str = r"    BonsaiDecodeCache             talker_decode_graphs;  // one Rust-owned completed workspace";

const OLD_INIT: &str =
    r"    pt->talker_decode_graphs.resize(((size_t) pt->talker_kv.max_seq_len + 255) / 256);";

const RUST_INIT: &str = r"    const bool decode_cache_ok = pt->talker_decode_graphs.init(((size_t) pt->talker_kv.max_seq_len + 255) / 256);";

const OLD_FREE: &str = r"    for (size_t g = 0; g < pt->talker_decode_graphs.size(); g++) {
        talker_decode_graph_free(&pt->talker_decode_graphs[g]);
    }
    pt->talker_decode_graphs.clear();";

const OLD_SIGNATURE: &str =
    r"                                  std::vector<TalkerDecodeGraph> & graphs,";

const RUST_SIGNATURE: &str =
    r"                                  BonsaiDecodeCache &             graphs,";

const BUILD_MARKER: &str =
    r"// Batched decode: append one position per KV set over the static graph";

#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent source-copy overlay selects these fixed transforms."
)]
pub(super) fn graph_type(text: &str) -> io::Result<String> {
    let original = identity(text, GRAPH_TYPE_BYTES, GRAPH_TYPE_SHA256)?;
    Ok(format!("{original}{NATIVE_HOLDER}"))
}

#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent source-copy overlay selects these fixed transforms."
)]
pub(super) fn forward(text: &str) -> io::Result<String> {
    let original = identity(text, FORWARD_BYTES, FORWARD_SHA256)?;
    patch_forward(&original)
}

fn patch_forward(text: &str) -> io::Result<String> {
    let patched = replace(text, OLD_SIGNATURE, RUST_SIGNATURE)?;
    let patched = replace(&patched, OLD_SELECTION, RUST_SELECTION)?;
    let patched = replace(
        &patched,
        BUILD_MARKER,
        &format!("{BUILD_CALLBACK}{BUILD_MARKER}"),
    )?;
    replace(
        &patched,
        "#include <vector>\n",
        "#include <vector>\n#include <new>\n",
    )
}

#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent source-copy overlay selects these fixed transforms."
)]
pub(super) fn pipeline_header(text: &str) -> io::Result<String> {
    let original = identity(text, PIPELINE_HEADER_BYTES, PIPELINE_HEADER_SHA256)?;
    replace(&original, OLD_FIELD, RUST_FIELD)
}

#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent source-copy overlay selects these fixed transforms."
)]
pub(super) fn pipeline(text: &str) -> io::Result<String> {
    let original = identity(text, PIPELINE_BYTES, PIPELINE_SHA256)?;
    patch_pipeline(&original)
}

fn patch_pipeline(text: &str) -> io::Result<String> {
    let patched = replace(text, OLD_INIT, RUST_INIT)?;
    let patched = replace(
        &patched,
        "    bool graphs_ok = graph_arena_init(",
        "    bool graphs_ok = decode_cache_ok && graph_arena_init(",
    )?;
    replace(&patched, OLD_FREE, "    pt->talker_decode_graphs.clear();")
}

#[cfg(test)]
mod tests {
    use super::{
        BUILD_CALLBACK, BUILD_MARKER, NATIVE_HOLDER, OLD_FIELD, OLD_FREE, OLD_INIT, OLD_SELECTION,
        OLD_SIGNATURE, RUST_SELECTION, forward, graph_type, patch_forward, patch_pipeline,
        pipeline, pipeline_header,
    };

    #[test]
    fn native_ownership_overlays_reject_source_drift() {
        for transform in [graph_type, forward, pipeline_header, pipeline] {
            assert!(transform("changed").is_err());
        }
    }

    #[test]
    fn graph_acquisition_preserves_upload_compute_and_readback_after_lease() -> std::io::Result<()>
    {
        let suffix =
            "upload_inputs();\nggml_backend_graph_compute(backend, tg->gf);\nread_outputs();\n";
        let original =
            format!("#include <vector>\n{BUILD_MARKER}\n{OLD_SIGNATURE}\n{OLD_SELECTION}{suffix}");
        let patched = patch_forward(&original)?;
        assert!(patched.contains(BUILD_CALLBACK));
        assert!(patched.ends_with(&format!("{RUST_SELECTION}{suffix}")));
        assert!(!patched.contains("std::vector<TalkerDecodeGraph>"));
        assert!(NATIVE_HOLDER.contains(
            "ggml_backend_synchronize(backend);\n        bonsai_qt_decode_cache_complete(cache);"
        ));
        assert!(NATIVE_HOLDER.contains("BonsaiDecodeCache(const BonsaiDecodeCache &) = delete;"));
        assert!(NATIVE_HOLDER.contains("~BonsaiDecodeCache() { clear(); }"));
        assert!(patch_forward(&patched).is_err());
        assert!(patch_forward(&original.repeat(2)).is_err());
        Ok(())
    }

    #[test]
    fn pipeline_init_failure_and_teardown_release_before_persistent_resources()
    -> std::io::Result<()> {
        let original = format!(
            "{OLD_INIT}\n    bool graphs_ok = graph_arena_init(...);\n    if (!graphs_ok) {{ pt->talker_decode_graphs.clear(); }}\n{OLD_FREE}\n    graph_arena_free(&pt->talker_arena);\n    kv_cache_free(&pt->talker_kv);\n"
        );
        let patched = patch_pipeline(&original)?;
        assert!(patched.contains("decode_cache_ok && graph_arena_init("));
        assert!(patched.ends_with("    pt->talker_decode_graphs.clear();\n    graph_arena_free(&pt->talker_arena);\n    kv_cache_free(&pt->talker_kv);\n"));
        assert!(!patched.contains("talker_decode_graph_free("));
        assert!(!patched.contains(OLD_FIELD));
        assert!(patch_pipeline(&patched).is_err());
        assert!(patch_pipeline(&original.repeat(2)).is_err());
        Ok(())
    }
}
