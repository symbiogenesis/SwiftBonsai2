//! Reviewed Rust sampler, CPU thread, graph-cache and SIMD private-source overlay.

use std::{fs, io, path::Path};

use sha2::{Digest as _, Sha256};

#[path = "build_tokenizer.rs"]
mod tokenizer_overlay;

const THREAD_COUNT: &str = "static int backend_cpu_n_threads(void) {\n    int n = (int) std::thread::hardware_concurrency() / 2;\n    return n > 0 ? n : 1;\n}\n";
const THREAD_OVERRIDE: &str = "static int backend_cpu_n_threads(void) {\n    return bonsai_qt_cpu_threads(std::thread::hardware_concurrency());\n}\n";
const DECODE_GRAPH: &str =
    "    TalkerDecodeGraph * tg = &graphs[(size_t) ((n_kv_pad + 255) / 256 - 1)];\n";
const DECODE_GRAPH_DECLARATIONS: &str = "#include <cstddef>\nextern \"C\" size_t bonsai_qt_decode_graph_index(int padded_tokens, size_t windows);\nextern \"C\" bool bonsai_qt_decode_graph_rebuild(bool has_context, int cached_width, int requested_width);\n";
const RUST_DECODE_GRAPH: &str = "    TalkerDecodeGraph * tg = &graphs[bonsai_qt_decode_graph_index(n_kv_pad, graphs.size())];\n";
const EVICT_DECODE_GRAPHS: &str =
    "    // Runner's CPU-only worker retains one decode workspace. The previous
    // ggml_backend_graph_compute completed and synchronized before returning;
    // KV, weights and hidden_bridge have independent persistent ownership.
    // Free obsolete windows before allocating a replacement, including when
    // a new request starts at an earlier window or changes batch width.
    if (bonsai_qt_decode_graph_rebuild(tg->ctx != nullptr, tg->N, N)) {
        for (auto & cached : graphs) {
            if (&cached != tg) { talker_decode_graph_free(&cached); }
        }
    }
";
type Transform = fn(&str) -> io::Result<String>;

// Match the exact LF-normalized bytes from the verified qwentts.cpp archive at
// 5916bf7c79dbb4e66fc059cdc507a79a23a28bb4 without retaining its implementation.
// Both sampler callers draw directly from the Rust scalar export.
const PHILOX_BYTES: usize = 3552;
const PHILOX_SHA256: [u8; 32] = [
    0x41, 0xa9, 0x55, 0xe4, 0x26, 0x7e, 0xba, 0x8d, 0x1c, 0x1a, 0x4b, 0x55, 0xa0, 0xb8, 0x96, 0x4c,
    0x4a, 0x7b, 0xd2, 0x8f, 0x38, 0x0f, 0xaa, 0xe5, 0xf9, 0x81, 0x15, 0x24, 0x03, 0xc8, 0x1d, 0xe7,
];
// Exact normalized identity of the retired pinned native tokenizer header.
const BPE_BYTES: usize = 22533;
const BPE_SHA256: [u8; 32] = [
    0x27, 0x31, 0xaa, 0xe2, 0x55, 0xbb, 0x95, 0xab, 0x93, 0x48, 0x35, 0xf1, 0x14, 0x91, 0x42, 0x9d,
    0x67, 0xc9, 0xd9, 0xf1, 0x2e, 0xa9, 0xf3, 0x3d, 0xbd, 0xb4, 0x5d, 0x67, 0x37, 0xee, 0x24, 0x03,
];
// This identifies the downloaded source; its probability pipeline remains
// native. Captured controls retain the retired checked-in copy's observations.
const SAMPLING_BYTES: usize = 6906;
const SAMPLING_SHA256: [u8; 32] = [
    0x98, 0x12, 0xa8, 0xb4, 0x28, 0xef, 0xff, 0xcf, 0xe3, 0xd8, 0x32, 0x40, 0x9f, 0x3e, 0xae, 0x09,
    0x2c, 0xb3, 0xbe, 0x90, 0xbf, 0xee, 0xfb, 0xf1, 0x41, 0xf7, 0x5d, 0xf0, 0x32, 0x0e, 0x22, 0x41,
];
const PHILOX_DECLARATION: &str = "#pragma once\n#include <cstdint>\n\n// Runner's allocation-free safe Rust Philox4x32-10 implementation.\nextern \"C\" float bonsai_qt_philox_uniform(int64_t seed, int64_t subsequence, uint32_t counter);\n";
const SEMANTIC_DRAW: &str = "    philox_uniform_fill(seed, philox_subseq, 0u, &u, 1);";
const PREDICTOR_DRAW: &str =
    "                philox_uniform_fill(s.seed, s.subseq_base + 1 + g, 0u, &u, 1);";

fn philox(text: &str) -> io::Result<String> {
    let normalized = text.replace("\r\n", "\n");
    if !matches_source(&normalized, PHILOX_BYTES, PHILOX_SHA256) {
        return Err(io::Error::other("Pinned Qwen Philox source differs."));
    }
    Ok(PHILOX_DECLARATION.to_owned())
}

fn matches_source(text: &str, expected_bytes: usize, expected_sha256: [u8; 32]) -> bool {
    if text.len() != expected_bytes {
        return false;
    }
    let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
    digest == expected_sha256
}

fn tokenizer(text: &str) -> io::Result<String> {
    let normalized = text.replace("\r\n", "\n");
    if !matches_source(&normalized, BPE_BYTES, BPE_SHA256) {
        return Err(io::Error::other("Pinned Qwen tokenizer source differs."));
    }
    Ok(tokenizer_overlay::SOURCE.to_owned())
}

#[cfg(test)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "Expose the generated header only to sibling model-free tokenizer tests."
)]
pub(super) const fn tokenizer_header_for_test() -> &'static str {
    tokenizer_overlay::SOURCE
}

fn semantic_sampling(text: &str) -> io::Result<String> {
    semantic_sampling_with_identity(text, SAMPLING_BYTES, SAMPLING_SHA256)
}

fn semantic_sampling_with_identity(
    text: &str,
    expected_bytes: usize,
    expected_sha256: [u8; 32],
) -> io::Result<String> {
    let normalized = text.replace("\r\n", "\n");
    if !matches_source(&normalized, expected_bytes, expected_sha256) {
        return Err(io::Error::other("Pinned Qwen sampler source differs."));
    }
    let text = normalized.as_str();
    let start = text
        .find("// Mask logits in [lo, hi)")
        .ok_or_else(|| io::Error::other("Pinned Qwen sampler source differs."))?;
    let end = text
        .find("// Stochastic sampler.")
        .ok_or_else(|| io::Error::other("Pinned Qwen sampler source differs."))?;
    let text = format!(
        "{}extern \"C\" int bonsai_qt_greedy_argmax(const float * logits, int V);\n{}{}",
        &text[..start],
        SAMPLING_HELPERS,
        &text[end..]
    );
    let text = replace_once(
        &text,
        "return (int) (std::max_element(logits, logits + V) - logits);",
        "return bonsai_qt_greedy_argmax(logits, V);",
    )?;
    replace_once(
        &text,
        SEMANTIC_DRAW,
        "    u = bonsai_qt_philox_uniform(seed, philox_subseq, 0u);",
    )
}

// Native code retains only reusable storage and delegates slice policy to Rust.
const SAMPLING_HELPERS: &str = "// Runner owns the masking and unique-history repetition policy.\nextern \"C\" void bonsai_qt_apply_suppress(float * logits, int V, int lo, int hi, int keep);\nextern \"C\" void bonsai_qt_apply_repetition_penalty(float * logits, int V, const int32_t * history, int n_history, float penalty, uint8_t * seen);\n\nstatic inline void apply_suppress(float * logits, int V, int lo, int hi, int keep) {\n    bonsai_qt_apply_suppress(logits, V, lo, hi, keep);\n}\n\nstatic inline void apply_repetition_penalty(float * logits, int V, const int32_t * history, int n_history, float penalty) {\n    if (penalty == 1.0f || n_history <= 0) { return; }\n    static thread_local std::vector<uint8_t> seen_buf;\n    seen_buf.resize((size_t) V);\n    bonsai_qt_apply_repetition_penalty(logits, V, history, n_history, penalty, seen_buf.data());\n}\n\n";

fn predictor_sampling(text: &str) -> io::Result<String> {
    replace_once(
        text,
        PREDICTOR_DRAW,
        "                u = bonsai_qt_philox_uniform(s.seed, s.subseq_base + 1 + g, 0u);",
    )
}

fn replace_once(text: &str, before: &str, after: &str) -> io::Result<String> {
    if text.matches(before).count() != 1 {
        return Err(io::Error::other("Pinned Qwen build overlay input differs."));
    }
    Ok(text.replacen(before, after, 1))
}

fn backend(text: &str) -> io::Result<String> {
    let text = replace_once(
        text,
        "#include <thread>",
        "#include <thread>\n\n// Runner owns bounded thread selection; the native call only queries the OS.\nextern \"C\" int bonsai_qt_cpu_threads(unsigned int logical_threads);",
    )?;
    replace_once(&text, THREAD_COUNT, THREAD_OVERRIDE)
}

fn vectors(text: &str) -> io::Result<String> {
    replace_once(
        text,
        "#include \"vec.h\"\n",
        "#include \"vec.h\"\n\n#if !defined(__aarch64__) || !defined(__ARM_NEON) || !defined(GGML_SIMD)\n#error Native Qwen requires ARM64 NEON vector kernels; scalar fallback is unsupported.\n#endif\n",
    )
}

fn decode_graphs(text: &str) -> io::Result<String> {
    // Keep the upstream allocation, failure cleanup, input uploads and compute
    // unchanged. Only scratch from completed, unselected windows is released.
    if text.contains(DECODE_GRAPH_DECLARATIONS) || text.contains(RUST_DECODE_GRAPH) {
        return Err(io::Error::other("Pinned Qwen build overlay input differs."));
    }
    let replacement = format!("{RUST_DECODE_GRAPH}{EVICT_DECODE_GRAPHS}");
    let patched = replace_once(text, DECODE_GRAPH, &replacement)?;
    Ok(format!("{DECODE_GRAPH_DECLARATIONS}{patched}"))
}

#[cfg(test)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "Expose generated glue only to the native graph-lifetime regression."
)]
pub(super) fn decode_graph_fixture_for_test() -> io::Result<String> {
    let patched = decode_graphs(DECODE_GRAPH)?;
    let (declarations, selection) = patched.split_at(DECODE_GRAPH_DECLARATIONS.len());
    Ok(include_str!("tests/decode_graph_cache.cpp")
        .replace("// BONSAI_DECODE_GRAPH_DECLARATIONS", declarations)
        .replace("// BONSAI_DECODE_GRAPH_SELECTION", selection))
}

/// Copy externally verified immutable source bytes, then overlay only that copy.
#[allow(
    clippy::redundant_pub_crate,
    reason = "Keep the reviewed build helper private to its build-script or test parent."
)]
pub(super) fn prepare(source: &Path, destination: &Path) -> io::Result<()> {
    prepare_with(source, destination, &TRANSFORMS)
}

const TRANSFORMS: [(&str, Transform); 7] = [
    ("src/backend.h", backend),
    ("ggml/src/ggml-cpu/vec.cpp", vectors),
    ("src/talker-forward.h", decode_graphs),
    ("src/philox.h", philox),
    ("src/sampling.h", semantic_sampling),
    ("src/sampling-graph.h", predictor_sampling),
    ("src/bpe.h", tokenizer),
];

fn prepare_with(
    source: &Path,
    destination: &Path,
    transforms: &[(&str, Transform)],
) -> io::Result<()> {
    copy(source, destination)?;
    for &(relative, transform) in transforms {
        let path = destination.join(relative);
        let original = fs::read_to_string(&path)?.replace("\r\n", "\n");
        fs::write(path, transform(&original)?)?;
    }
    Ok(())
}

fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if redirected(&metadata) {
            return Err(io::Error::other("Pinned Qwen source contains a redirect."));
        }
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy(&entry.path(), &target)?;
        } else if metadata.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::other(
                "Pinned Qwen source contains a special file.",
            ));
        }
    }
    Ok(())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DECODE_GRAPH, DECODE_GRAPH_DECLARATIONS, EVICT_DECODE_GRAPHS, PHILOX_DECLARATION,
        PREDICTOR_DRAW, RUST_DECODE_GRAPH, SEMANTIC_DRAW, THREAD_COUNT, THREAD_OVERRIDE,
        TRANSFORMS, backend, decode_graphs, matches_source, philox, predictor_sampling,
        prepare_with, semantic_sampling, semantic_sampling_with_identity, tokenizer, vectors,
    };

    // Published SHA-256 known-answer vector; contains no native source code.
    const FIXTURE_SOURCE: &str = "abc";
    const FIXTURE_SHA256: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];

    // A minimal transform/compile scaffold, never a probability implementation
    // or a numerical reference. Production accepts only the pinned source hash.
    const SAMPLER_FIXTURE: &str = r#"#pragma once
#include "philox.h"
#include <algorithm>
#include <cstdint>
#include <vector>
// Mask logits in [lo, hi)
// Synthetic removable source markers: if (lo < 0), s * penalty.
// Stochastic sampler.
static int sample_top_k_p(float * logits, int V, float temperature,
                          int64_t seed, int64_t philox_subseq) {
    if (temperature <= 0.0f) {
        return (int) (std::max_element(logits, logits + V) - logits);
    }
    std::vector<float> tmp(logits, logits + V);
    std::nth_element(tmp.begin(), tmp.begin(), tmp.end());
    std::vector<float> sorted_buf(tmp);
    std::sort(sorted_buf.begin(), sorted_buf.end());
    float u = 0.0f;
    philox_uniform_fill(seed, philox_subseq, 0u, &u, 1);
    return u > 0.0f ? 0 : V - 1;
}
"#;
    const SAMPLER_FIXTURE_SHA256: [u8; 32] = [
        0xcc, 0x28, 0x74, 0x46, 0xd8, 0xe1, 0x17, 0x46, 0xb4, 0x79, 0xfc, 0x96, 0x01, 0xcd, 0xdc,
        0x30, 0x41, 0x61, 0xe8, 0x36, 0x3a, 0xb9, 0x26, 0xa7, 0x97, 0x70, 0xdf, 0xe6, 0xb1, 0x58,
        0x28, 0x1c,
    ];

    fn fixture_semantic_sampling(text: &str) -> std::io::Result<String> {
        semantic_sampling_with_identity(text, SAMPLER_FIXTURE.len(), SAMPLER_FIXTURE_SHA256)
    }

    fn fixture_philox(text: &str) -> std::io::Result<String> {
        if !matches_source(text, FIXTURE_SOURCE.len(), FIXTURE_SHA256) {
            return Err(std::io::Error::other("Test source fixture differs."));
        }
        Ok(PHILOX_DECLARATION.to_owned())
    }

    fn fixture_tokenizer(text: &str) -> std::io::Result<String> {
        if !matches_source(text, FIXTURE_SOURCE.len(), FIXTURE_SHA256) {
            return Err(std::io::Error::other("Test tokenizer fixture differs."));
        }
        Ok(super::tokenizer_header_for_test().to_owned())
    }

    #[test]
    fn overlay_preserves_the_immutable_source_cache() -> std::io::Result<()> {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let scratch = std::env::temp_dir().join(format!(
            "Bonsai-Qwen-Overlay-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(std::io::Error::other)?
                .as_nanos()
        ));
        let source = scratch.join("original");
        fs::create_dir_all(source.join("src"))?;
        let header = format!("#include <thread>\n{THREAD_COUNT}\n");
        fs::write(source.join("src/backend.h"), &header)?;
        fs::write(source.join("src/qwen.cpp"), "extern \"C\" {\n}")?;
        fs::write(source.join("src/talker-forward.h"), DECODE_GRAPH)?;
        fs::write(source.join("src/philox.h"), FIXTURE_SOURCE)?;
        fs::write(source.join("src/sampling.h"), SAMPLER_FIXTURE)?;
        fs::write(source.join("src/bpe.h"), FIXTURE_SOURCE)?;
        fs::write(source.join("src/sampling-graph.h"), PREDICTOR_DRAW)?;
        fs::create_dir_all(source.join("ggml/src/ggml-cpu"))?;
        fs::write(
            source.join("ggml/src/ggml-cpu/vec.cpp"),
            "#include \"vec.h\"\n",
        )?;
        let copy = scratch.join("patched");
        let mut transforms = TRANSFORMS;
        for (relative, transform) in &mut transforms {
            if *relative == "src/philox.h" {
                *transform = fixture_philox;
            } else if *relative == "src/bpe.h" {
                *transform = fixture_tokenizer;
            } else if *relative == "src/sampling.h" {
                *transform = fixture_semantic_sampling;
            }
        }
        prepare_with(&source, &copy, &transforms)?;
        assert_eq!(fs::read_to_string(source.join("src/backend.h"))?, header);
        assert!(fs::read_to_string(copy.join("src/backend.h"))?.contains("bonsai_qt_cpu_threads"));
        assert_eq!(
            fs::read_to_string(copy.join("src/qwen.cpp"))?,
            fs::read_to_string(source.join("src/qwen.cpp"))?
        );
        assert_eq!(
            fs::read_to_string(source.join("ggml/src/ggml-cpu/vec.cpp"))?,
            "#include \"vec.h\"\n"
        );
        assert!(
            fs::read_to_string(copy.join("ggml/src/ggml-cpu/vec.cpp"))?
                .contains("!defined(GGML_SIMD)")
        );
        assert_eq!(
            fs::read_to_string(source.join("src/talker-forward.h"))?,
            DECODE_GRAPH
        );
        assert!(
            fs::read_to_string(copy.join("src/talker-forward.h"))?.contains(EVICT_DECODE_GRAPHS)
        );
        assert_eq!(
            fs::read_to_string(source.join("src/philox.h"))?,
            FIXTURE_SOURCE
        );
        assert_eq!(
            fs::read_to_string(copy.join("src/philox.h"))?,
            PHILOX_DECLARATION
        );
        for relative in ["src/sampling.h", "src/sampling-graph.h"] {
            assert!(fs::read_to_string(source.join(relative))?.contains("philox_uniform_fill"));
            assert!(fs::read_to_string(copy.join(relative))?.contains("bonsai_qt_philox_uniform"));
        }
        assert_eq!(
            fs::read_to_string(source.join("src/bpe.h"))?,
            FIXTURE_SOURCE
        );
        let tokenizer = fs::read_to_string(copy.join("src/bpe.h"))?;
        assert!(tokenizer.contains("bonsai_qt_bpe_encode"));
        assert!(!tokenizer.contains("std::unordered_map"));
        // The synthetic hash fixture is accepted only by the private test
        // transform, never by the fixed production Philox identity.
        assert!(super::prepare(&source, &scratch.join("rejected-production-fixture")).is_err());
        fs::remove_dir_all(scratch)?;
        Ok(())
    }

    #[test]
    fn tokenizer_overlay_removes_native_policy_and_rejects_source_drift() -> std::io::Result<()> {
        let patched = fixture_tokenizer(FIXTURE_SOURCE)?;
        assert!(patched.contains("gguf_get_arr_str"));
        assert!(patched.contains("~BPETokenizer() { bonsai_qt_bpe_free(owner); }"));
        assert!(patched.contains("BPETokenizer(const BPETokenizer &) = delete;"));
        assert!(patched.contains("bonsai_qt_bpe_encode(tok->owner,"));
        for policy in [
            "std::unordered_map",
            "gpt2_pre_tokenize",
            "byte_level_encode",
            "bpe_merge(",
            "id_to_str",
            "specials.emplace_back",
        ] {
            assert!(!patched.contains(policy));
        }
        assert!(fixture_tokenizer("abd").is_err());
        assert!(tokenizer(FIXTURE_SOURCE).is_err());
        assert!(tokenizer(&"x".repeat(super::BPE_BYTES)).is_err());
        assert!(tokenizer(&patched).is_err());
        assert!(tokenizer("changed").is_err());
        assert!(fixture_tokenizer(&FIXTURE_SOURCE.repeat(2)).is_err());
        Ok(())
    }

    #[test]
    fn rust_philox_overlay_removes_native_algorithm_and_rejects_source_drift() -> std::io::Result<()>
    {
        assert!(!PHILOX_DECLARATION.contains("philox4x32_10"));
        assert!(!PHILOX_DECLARATION.contains("philox_uniform_fill"));
        for changed in [PHILOX_DECLARATION, "changed", &FIXTURE_SOURCE.repeat(2)] {
            assert!(philox(changed).is_err());
        }
        let draws: [(super::Transform, &str); 2] = [
            (fixture_semantic_sampling, SAMPLER_FIXTURE),
            (predictor_sampling, PREDICTOR_DRAW),
        ];
        for (transform, draw) in draws {
            let original = if draw == SAMPLER_FIXTURE {
                draw.to_owned()
            } else {
                format!("before\n{draw}\nafter\n")
            };
            let patched = transform(&original)?;
            if draw != SAMPLER_FIXTURE {
                assert!(patched.starts_with("before\n") && patched.ends_with("\nafter\n"));
            }
            if draw == SAMPLER_FIXTURE {
                assert!(!patched.contains(SEMANTIC_DRAW));
            } else {
                assert!(!patched.contains("philox_uniform_fill"));
            }
            assert!(patched.contains("bonsai_qt_philox_uniform"));
            assert!(transform(&patched).is_err());
            assert!(transform("changed").is_err());
            assert!(transform(&original.repeat(2)).is_err());
        }
        Ok(())
    }

    #[test]
    fn source_identity_guard_checks_exact_length_hash_and_line_endings() {
        assert!(matches_source(FIXTURE_SOURCE, 3, FIXTURE_SHA256));
        assert!(!matches_source("abd", 3, FIXTURE_SHA256));
        assert!(!matches_source(FIXTURE_SOURCE, 2, FIXTURE_SHA256));
        assert!(!matches_source(FIXTURE_SOURCE, 4, FIXTURE_SHA256));
        assert!(!matches_source("a\r\nbc", 3, FIXTURE_SHA256));
        let mut digest = FIXTURE_SHA256;
        digest[31] ^= 1;
        assert!(!matches_source(FIXTURE_SOURCE, 3, digest));
        assert!(philox(&"x".repeat(super::PHILOX_BYTES)).is_err());
    }

    #[test]
    fn sampler_helpers_move_policy_to_rust_and_reject_source_drift() -> std::io::Result<()> {
        let patched = fixture_semantic_sampling(SAMPLER_FIXTURE)?;
        assert!(patched.contains("bonsai_qt_apply_suppress(logits, V, lo, hi, keep);"));
        assert!(patched.contains("bonsai_qt_apply_repetition_penalty(logits, V, history, n_history, penalty, seen_buf.data());"));
        assert!(!patched.contains("logits[i] = -INFINITY;\n    }\n}"));
        assert!(!patched.contains("seen_buf[(size_t) tok]"));
        assert!(!patched.contains("std::max_element"));
        assert!(patched.contains("return bonsai_qt_greedy_argmax(logits, V);"));
        assert!(patched.contains("std::nth_element"));
        assert!(patched.contains("std::sort(sorted_buf.begin()"));
        assert!(patched.contains("u = bonsai_qt_philox_uniform(seed, philox_subseq, 0u);"));
        assert!(
            fixture_semantic_sampling(&SAMPLER_FIXTURE.replace("if (lo < 0)", "if (lo <= 0)"))
                .is_err()
        );
        assert!(
            fixture_semantic_sampling(&SAMPLER_FIXTURE.replace("s * penalty", "s / penalty"))
                .is_err()
        );
        assert!(fixture_semantic_sampling(&patched).is_err());
        assert!(fixture_semantic_sampling("changed").is_err());
        assert!(fixture_semantic_sampling(&SAMPLER_FIXTURE.repeat(2)).is_err());
        assert!(semantic_sampling(SAMPLER_FIXTURE).is_err());
        assert!(semantic_sampling(&"x".repeat(super::SAMPLING_BYTES)).is_err());
        assert!(matches_source(
            SAMPLER_FIXTURE,
            SAMPLER_FIXTURE.len(),
            SAMPLER_FIXTURE_SHA256
        ));
        let crlf = SAMPLER_FIXTURE.replace('\n', "\r\n");
        assert_eq!(fixture_semantic_sampling(&crlf)?, patched);
        Ok(())
    }

    #[test]
    fn generated_sampler_scaffold_compiles_with_private_rust_declarations()
    -> Result<(), Box<dyn std::error::Error>> {
        // This validates transformed Rust-call wiring only. The complete pinned
        // source build/link remains an owning hosted integration gate.
        use std::{
            env, fs,
            path::PathBuf,
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };

        let compiler = PathBuf::from(
            env::var_os("BONSAI_TTS_COMPILER")
                .or_else(|| option_env!("BONSAI_TTS_COMPILER").map(Into::into))
                .ok_or(
                    "Set the provisioned BONSAI_TTS_COMPILER for the sampler build regression.",
                )?,
        );
        assert!(compiler.is_absolute() && compiler.is_file());
        let working = env::temp_dir().join(format!(
            "Bonsai-Qwen-SamplerBuild-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
        fs::create_dir(&working)?;
        fs::write(working.join("philox.h"), PHILOX_DECLARATION)?;
        fs::write(
            working.join("sampling.h"),
            fixture_semantic_sampling(SAMPLER_FIXTURE)?,
        )?;
        let source = working.join("probe.cpp");
        fs::write(
            &source,
            "#include \"sampling.h\"\nvoid probe(float * logits, int V, const int32_t * history, int count) {\n    (void) &sample_top_k_p;\n    apply_suppress(logits, V, -1, V, 2);\n    apply_repetition_penalty(logits, V, history, count, 1.05f);\n    (void) sample_top_k_p(logits, V, 0.0f, 0, 0);\n    (void) sample_top_k_p(logits, V, 1.0f, 0, 0);\n}\n",
        )?;
        let compile = Command::new(compiler)
            .args(["/nologo", "/std:c++17", "/EHsc", "/W4", "/WX", "/c"])
            .arg(source)
            .arg(format!("/Fo{}", working.join("probe.obj").display()))
            .current_dir(&working)
            .output()?;
        fs::write(working.join("compiler.stdout"), &compile.stdout)?;
        fs::write(working.join("compiler.stderr"), &compile.stderr)?;
        assert!(
            compile.status.success(),
            "Generated sampler scaffold failed to compile; retained {}",
            working.display()
        );
        fs::remove_dir_all(&working)?;
        Ok(())
    }

    #[test]
    fn thread_overlay_routes_native_queries_to_rust_and_rejects_source_drift() -> std::io::Result<()>
    {
        let source = format!("#include <thread>\n{THREAD_COUNT}\nremaining backend\n");
        let patched = backend(&source)?;
        assert!(
            patched
                .contains("extern \"C\" int bonsai_qt_cpu_threads(unsigned int logical_threads);")
        );
        assert!(patched.contains(THREAD_OVERRIDE));
        assert!(patched.ends_with("remaining backend\n"));
        assert!(!patched.contains("std::atomic") && !patched.contains("std::memory_order"));
        assert!(!patched.contains("bonsai_qt_set_cpu_threads"));
        assert!(backend(&patched).is_err());
        assert!(backend(&source.replace(THREAD_COUNT, "changed")).is_err());
        assert!(backend(&source.replace("return n > 0 ? n : 1;", "return n;")).is_err());
        assert!(backend(&source.replace("#include <thread>", "changed")).is_err());
        assert!(backend(&source.repeat(2)).is_err());
        Ok(())
    }

    #[test]
    fn vector_guard_rejects_changed_inputs_and_requires_active_neon_kernels() -> std::io::Result<()>
    {
        let source = "#include \"vec.h\"\nrest of vector kernels\n";
        let patched = vectors(source)?;
        assert!(patched.contains("!defined(__aarch64__)"));
        assert!(patched.contains("!defined(__ARM_NEON)"));
        assert!(patched.contains("!defined(GGML_SIMD)"));
        assert!(patched.contains("rest of vector kernels"));
        assert!(vectors("changed input").is_err());
        assert!(vectors(&source.repeat(2)).is_err());
        Ok(())
    }

    #[test]
    fn decode_workspace_overlay_precedes_allocation_and_preserves_graph_execution()
    -> std::io::Result<()> {
        let suffix = "    if (tg->ctx && tg->N != N) { talker_decode_graph_free(tg); }\n    if (!tg->ctx && !talker_decode_graph_build(...)) { return false; }\n    upload_inputs();\n    ggml_backend_graph_compute(backend, tg->gf);\n";
        let source = format!("unchanged graph builder\n{DECODE_GRAPH}{suffix}");
        let expected = format!(
            "{DECODE_GRAPH_DECLARATIONS}unchanged graph builder\n{RUST_DECODE_GRAPH}{EVICT_DECODE_GRAPHS}{suffix}"
        );
        assert_eq!(decode_graphs(&source)?, expected);
        assert!(decode_graphs(&expected).is_err());
        assert!(decode_graphs("changed input").is_err());
        assert!(decode_graphs(&source.repeat(2)).is_err());
        Ok(())
    }
}
