//! Reviewed Rust sampler, CPU thread, graph-cache and SIMD private-source overlay.

use std::{fs, io, path::Path};

const THREAD_COUNT: &str = "static int backend_cpu_n_threads(void) {\n    int n = (int) std::thread::hardware_concurrency() / 2;\n    return n > 0 ? n : 1;\n}\n";
const THREAD_OVERRIDE: &str = "static int backend_cpu_n_threads(void) {\n    return bonsai_qt_cpu_threads(std::thread::hardware_concurrency());\n}\n";
const DECODE_GRAPH: &str =
    "    TalkerDecodeGraph * tg = &graphs[(size_t) ((n_kv_pad + 255) / 256 - 1)];\n";
const EVICT_DECODE_GRAPHS: &str =
    "    // Runner's CPU-only worker retains one decode workspace. The previous
    // ggml_backend_graph_compute completed and synchronized before returning;
    // KV, weights and hidden_bridge have independent persistent ownership.
    // Free obsolete windows before allocating a replacement, including when
    // a new request starts at an earlier window or changes batch width.
    if (!tg->ctx || tg->N != N) {
        for (auto & cached : graphs) {
            if (&cached != tg) { talker_decode_graph_free(&cached); }
        }
    }
";
type Transform = fn(&str) -> io::Result<String>;

// Retain the pinned native implementation only as an independent test oracle
// and exact build anchor. Production replaces the entire header with this
// declaration; both sampler callers draw directly from the Rust scalar export.
const PHILOX_REFERENCE: &str = include_str!("tests/reference/philox.h");
const PHILOX_DECLARATION: &str = "#pragma once\n#include <cstdint>\n\n// Runner's allocation-free safe Rust Philox4x32-10 implementation.\nextern \"C\" float bonsai_qt_philox_uniform(int64_t seed, int64_t subsequence, uint32_t counter);\n";
const SEMANTIC_DRAW: &str = "    philox_uniform_fill(seed, philox_subseq, 0u, &u, 1);";
const PREDICTOR_DRAW: &str =
    "                philox_uniform_fill(s.seed, s.subseq_base + 1 + g, 0u, &u, 1);";

fn philox(text: &str) -> io::Result<String> {
    if text.replace("\r\n", "\n") != PHILOX_REFERENCE.replace("\r\n", "\n") {
        return Err(io::Error::other("Pinned Qwen Philox source differs."));
    }
    Ok(PHILOX_DECLARATION.to_owned())
}

fn semantic_sampling(text: &str) -> io::Result<String> {
    replace_once(
        text,
        SEMANTIC_DRAW,
        "    u = bonsai_qt_philox_uniform(seed, philox_subseq, 0u);",
    )
}

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
    if text.contains(EVICT_DECODE_GRAPHS) {
        return Err(io::Error::other("Pinned Qwen build overlay input differs."));
    }
    let replacement = format!("{DECODE_GRAPH}{EVICT_DECODE_GRAPHS}");
    replace_once(text, DECODE_GRAPH, &replacement)
}

/// Copy externally verified immutable source bytes, then overlay only that copy.
#[allow(
    clippy::redundant_pub_crate,
    reason = "Keep the reviewed build helper private to its build-script or test parent."
)]
pub(super) fn prepare(source: &Path, destination: &Path) -> io::Result<()> {
    copy(source, destination)?;
    let transforms: [(&str, Transform); 6] = [
        ("src/backend.h", backend),
        ("ggml/src/ggml-cpu/vec.cpp", vectors),
        ("src/talker-forward.h", decode_graphs),
        ("src/philox.h", philox),
        ("src/sampling.h", semantic_sampling),
        ("src/sampling-graph.h", predictor_sampling),
    ];
    for (relative, transform) in transforms {
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
        DECODE_GRAPH, EVICT_DECODE_GRAPHS, PHILOX_DECLARATION, PHILOX_REFERENCE, PREDICTOR_DRAW,
        SEMANTIC_DRAW, THREAD_COUNT, THREAD_OVERRIDE, backend, decode_graphs, philox,
        predictor_sampling, prepare, semantic_sampling, vectors,
    };

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
        fs::write(source.join("src/philox.h"), PHILOX_REFERENCE)?;
        fs::write(source.join("src/sampling.h"), SEMANTIC_DRAW)?;
        fs::write(source.join("src/sampling-graph.h"), PREDICTOR_DRAW)?;
        fs::create_dir_all(source.join("ggml/src/ggml-cpu"))?;
        fs::write(
            source.join("ggml/src/ggml-cpu/vec.cpp"),
            "#include \"vec.h\"\n",
        )?;
        let copy = scratch.join("patched");
        prepare(&source, &copy)?;
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
            PHILOX_REFERENCE
        );
        assert_eq!(
            fs::read_to_string(copy.join("src/philox.h"))?,
            PHILOX_DECLARATION
        );
        for relative in ["src/sampling.h", "src/sampling-graph.h"] {
            assert!(fs::read_to_string(source.join(relative))?.contains("philox_uniform_fill"));
            assert!(fs::read_to_string(copy.join(relative))?.contains("bonsai_qt_philox_uniform"));
        }
        fs::remove_dir_all(scratch)?;
        Ok(())
    }

    #[test]
    fn rust_philox_overlay_removes_native_algorithm_and_rejects_source_drift() -> std::io::Result<()>
    {
        assert_eq!(philox(PHILOX_REFERENCE)?, PHILOX_DECLARATION);
        assert!(!PHILOX_DECLARATION.contains("philox4x32_10"));
        assert!(!PHILOX_DECLARATION.contains("philox_uniform_fill"));
        for changed in [PHILOX_DECLARATION, "changed", &PHILOX_REFERENCE.repeat(2)] {
            assert!(philox(changed).is_err());
        }
        let draws: [(super::Transform, &str); 2] = [
            (semantic_sampling, SEMANTIC_DRAW),
            (predictor_sampling, PREDICTOR_DRAW),
        ];
        for (transform, draw) in draws {
            let original = format!("before\n{draw}\nafter\n");
            let patched = transform(&original)?;
            assert!(patched.starts_with("before\n") && patched.ends_with("\nafter\n"));
            assert!(!patched.contains("philox_uniform_fill"));
            assert!(patched.contains("bonsai_qt_philox_uniform"));
            assert!(transform(&patched).is_err());
            assert!(transform("changed").is_err());
            assert!(transform(&original.repeat(2)).is_err());
        }
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
        let expected =
            format!("unchanged graph builder\n{DECODE_GRAPH}{EVICT_DECODE_GRAPHS}{suffix}");
        assert_eq!(decode_graphs(&source)?, expected);
        assert!(decode_graphs(&expected).is_err());
        assert!(decode_graphs("changed input").is_err());
        assert!(decode_graphs(&source.repeat(2)).is_err());
        Ok(())
    }

    #[test]
    fn generated_decode_workspace_eviction_releases_only_completed_graphs()
    -> Result<(), Box<dyn std::error::Error>> {
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
                    "Set the provisioned BONSAI_TTS_COMPILER for the native cache regression.",
                )?,
        );
        assert!(compiler.is_absolute() && compiler.is_file());
        assert!(!compiler.to_string_lossy().contains('%'));
        let working = env::temp_dir().join(format!(
            "Bonsai-Qwen-GraphCache-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
        fs::create_dir(&working)?;
        let fixture = include_str!("tests/decode_graph_cache.cpp");
        let source = fixture.replace(
            "// BONSAI_DECODE_GRAPH_SELECTION",
            &decode_graphs(DECODE_GRAPH)?,
        );
        let source_path = working.join("cache.cpp");
        let executable = working.join("cache.exe");
        fs::write(&source_path, source)?;
        let output = Command::new(compiler)
            .args(["/nologo", "/std:c++17", "/EHsc", "/W4", "/WX", "/UNDEBUG"])
            .arg(&source_path)
            .arg(format!("/Fe{}", executable.display()))
            .current_dir(&working)
            .output()?;
        fs::write(working.join("compiler.stdout"), &output.stdout)?;
        fs::write(working.join("compiler.stderr"), &output.stderr)?;
        assert!(
            output.status.success(),
            "Native cache fixture failed to compile; retained {}",
            working.display()
        );
        assert!(
            Command::new(&executable)
                .current_dir(&working)
                .status()?
                .success()
        );
        fs::remove_dir_all(&working)?;
        Ok(())
    }
}
