//! Safe Rust ownership of assertions from pinned, MIT-licensed Prism tests.
//!
//! Both targets run the same Rust inputs, reference calculations and assertions.
//! The C++ target delegates production operations to a separate native bridge.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Sibling modules share adapters explicitly; unreachable_pub rejects unrestricted pub."
)]

#[path = "upstream_prism/allocation.rs"]
mod allocation;
#[path = "upstream_prism/architectures.rs"]
mod architectures;
#[path = "upstream_prism/arg_parser.rs"]
mod arg_parser;
#[path = "upstream_prism/chat.rs"]
mod chat;
#[path = "upstream_prism/chat_analysis.rs"]
mod chat_analysis;
#[path = "upstream_prism/chat_auto.rs"]
mod chat_auto;
#[path = "upstream_prism/chat_peg.rs"]
mod chat_peg;
#[path = "upstream_prism/chat_template.rs"]
mod chat_template;
#[path = "upstream_prism/gguf.rs"]
mod gguf;
#[path = "upstream_prism/gguf_model_data.rs"]
mod gguf_model_data;
#[path = "upstream_prism/model_lifecycle.rs"]
mod model_lifecycle;
#[path = "upstream_prism/model_resolution.rs"]
mod model_resolution;
#[path = "upstream_prism/model_session.rs"]
mod model_session;
#[path = "upstream_prism/mtmd_chunks.rs"]
mod mtmd_chunks;
#[path = "upstream_prism/sampling.rs"]
mod sampling;
#[path = "upstream_prism/tokenizer.rs"]
mod tokenizer;
#[path = "upstream_prism/tokenizer_repository.rs"]
mod tokenizer_repository;

#[path = "upstream_prism/backend_ops.rs"]
mod backend_ops;
#[path = "upstream_prism/backend_sampler.rs"]
mod backend_sampler;
#[path = "upstream_prism/barrier.rs"]
mod barrier;
#[path = "upstream_prism/batch_allocator.rs"]
mod batch_allocator;
#[path = "upstream_prism/codec_assertions.rs"]
mod codec_assertions;
#[path = "upstream_prism/common.rs"]
mod common;
#[path = "upstream_prism/double_float.rs"]
mod double_float;
#[path = "upstream_prism/dspark_forward.rs"]
mod dspark_forward;
#[path = "upstream_prism/dspark_inputs.rs"]
mod dspark_inputs;
#[path = "upstream_prism/dspark_logsnr.rs"]
mod dspark_logsnr;
#[path = "upstream_prism/dspark_loop.rs"]
mod dspark_loop;
#[path = "upstream_prism/dspark_pair.rs"]
mod dspark_pair;
#[path = "upstream_prism/dspark_real.rs"]
mod dspark_real;
#[path = "upstream_prism/dspark_real_inputs.rs"]
mod dspark_real_inputs;
#[path = "upstream_prism/dspark_reference.rs"]
mod dspark_reference;
#[path = "upstream_prism/engine.rs"]
mod engine;
#[path = "upstream_prism/engine_codec.rs"]
mod engine_codec;
#[path = "upstream_prism/engine_vocab.rs"]
mod engine_vocab;
#[path = "upstream_prism/export_graph.rs"]
mod export_graph;
#[path = "upstream_prism/fixture.rs"]
mod fixture;
#[path = "upstream_prism/generated_inputs.rs"]
mod generated_inputs;
#[path = "upstream_prism/grammar_integration.rs"]
mod grammar_integration;
#[path = "upstream_prism/grammar_parser.rs"]
mod grammar_parser;
#[path = "upstream_prism/jinja.rs"]
mod jinja;
#[path = "upstream_prism/json_schema.rs"]
mod json_schema;
#[path = "upstream_prism/kv_mean_center.rs"]
mod kv_mean_center;
#[path = "upstream_prism/llama_grammar.rs"]
mod llama_grammar;
#[path = "upstream_prism/llguidance.rs"]
mod llguidance;
#[path = "upstream_prism/logging.rs"]
mod logging;
#[path = "upstream_prism/lora_conversion.rs"]
mod lora_conversion;
#[path = "upstream_prism/lora_convert.rs"]
mod lora_convert;
#[path = "upstream_prism/lora_gguf.rs"]
mod lora_gguf;
#[path = "upstream_prism/lora_inputs.rs"]
mod lora_inputs;
#[path = "upstream_prism/lora_vocab.rs"]
mod lora_vocab;
#[path = "upstream_prism/moe_reduce.rs"]
mod moe_reduce;
#[path = "upstream_prism/mtmd.rs"]
mod mtmd;
#[path = "upstream_prism/numerics.rs"]
mod numerics;
#[path = "upstream_prism/optimizer.rs"]
mod optimizer;
#[path = "upstream_prism/peg.rs"]
mod peg;
#[path = "upstream_prism/pq2.rs"]
mod pq2;
#[path = "upstream_prism/ptq1.rs"]
mod ptq1;
#[path = "upstream_prism/public_c.rs"]
mod public_c;
#[path = "upstream_prism/quant_selection.rs"]
mod quant_selection;
#[path = "upstream_prism/reasoning_budget.rs"]
mod reasoning_budget;
#[path = "upstream_prism/recurrent_rollback.rs"]
mod recurrent_rollback;
#[path = "upstream_prism/schema_python.rs"]
mod schema_python;
#[path = "upstream_prism/tokenizer_roundtrip.rs"]
mod tokenizer_roundtrip;
#[path = "upstream_prism/validator.rs"]
mod validator;
#[path = "upstream_prism/windows_random.rs"]
mod windows_random;

use std::env;
use std::fs;
use std::path::PathBuf;

use engine::{Engine, Target};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const UPSTREAM: &str = "b87ca7225456b06b0975b8239c66b5aecf07e37c";
const PQ2_ID: &str = "tests/test-pq2-row-shapes.cpp";
const PTQ1_ID: &str = "tests/test-ptq1_0-element-map.cpp";
const UNICODE_ID: &str = "tests/test-unicode.cpp";
const MTP_ID: &str = "tests/test-mtp-catchup-batch.cpp";
const MTMD_ID: &str = "tests/test-mtmd-impl.cpp";
#[derive(Clone, Copy, PartialEq)]
enum Bridge {
    Independent,
    Ggml,
    Common,
}

struct Case {
    selector: &'static str,
    id: &'static str,
    bridge: Bridge,
    run: fn(&Engine) -> Result<Value>,
}

const fn register(
    selector: &'static str,
    id: &'static str,
    bridge: Bridge,
    run: fn(&Engine) -> Result<Value>,
) -> Case {
    Case {
        selector,
        id,
        bridge,
        run,
    }
}

const CASES: &[Case] = &[
    register(
        "test-backend-ops",
        "tests/test-backend-ops.cpp",
        Bridge::Common,
        backend_ops::run,
    ),
    register(
        "test-tokenizers-repo",
        "tests/test-tokenizers-repo.sh",
        Bridge::Common,
        tokenizer_repository::run,
    ),
    register(
        "test-lora-conversion-inference",
        "tests/test-lora-conversion-inference.sh",
        Bridge::Common,
        lora_conversion::run,
    ),
    register(
        "test-backend-sampler",
        "tests/test-backend-sampler.cpp",
        Bridge::Common,
        backend_sampler::run,
    ),
    register(
        "test-export-graph-ops",
        "tests/test-export-graph-ops.cpp",
        Bridge::Common,
        export_graph::run,
    ),
    register(
        "test-dspark-forward",
        "tests/test-dspark-forward.cpp",
        Bridge::Common,
        dspark_forward::run,
    ),
    register(
        "test-dspark-loop",
        "tests/test-dspark-loop.cpp",
        Bridge::Common,
        dspark_loop::run,
    ),
    register(
        "test-dspark-real-eval",
        "tests/test-dspark-real-eval.cpp",
        Bridge::Common,
        dspark_real::run,
    ),
    register(
        "test-grammar-llguidance",
        "tests/test-grammar-llguidance.cpp",
        Bridge::Common,
        llguidance::run,
    ),
    register(
        "test-dspark-logsnr-meta",
        "tests/test-dspark-logsnr-meta.cpp",
        Bridge::Common,
        dspark_logsnr::run,
    ),
    register(
        "test-gguf-model-data",
        "tests/test-gguf-model-data.cpp",
        Bridge::Independent,
        quant_selection::run_metadata,
    ),
    register(
        "test-quant-type-selection",
        "tests/test-quant-type-selection.cpp",
        Bridge::Common,
        quant_selection::run_quantization,
    ),
    register(
        "test-barrier",
        "tests/test-barrier.cpp",
        Bridge::Common,
        barrier::run,
    ),
    register(
        "test-llama-archs",
        "tests/test-llama-archs.cpp",
        Bridge::Common,
        architectures::run,
    ),
    register(
        "test-recurrent-state-rollback",
        "tests/test-recurrent-state-rollback.cpp",
        Bridge::Common,
        recurrent_rollback::run,
    ),
    register(
        "test-opt",
        "tests/test-opt.cpp",
        Bridge::Common,
        optimizer::run,
    ),
    register(
        "test-kv-mean-center",
        "tests/test-kv-mean-center.cpp",
        Bridge::Common,
        kv_mean_center::run,
    ),
    register(
        "test-gbnf-validator",
        "tests/test-gbnf-validator.cpp",
        Bridge::Common,
        validator::run,
    ),
    register(
        "test-arg-parser",
        "tests/test-arg-parser.cpp",
        Bridge::Common,
        arg_parser::run,
    ),
    register(
        "test-chat",
        "tests/test-chat.cpp",
        Bridge::Common,
        chat::run,
    ),
    register(
        "test-model-resolution",
        "tests/test-model-resolution.cpp",
        Bridge::Common,
        model_resolution::run,
    ),
    register(
        "test-double-float",
        "tests/test-double-float.cpp",
        Bridge::Independent,
        double_float::run,
    ),
    register(
        "test-chat-analysis",
        "tests/test-chat-analysis.cpp",
        Bridge::Common,
        chat_analysis::run,
    ),
    register("test-c", "tests/test-c.c", Bridge::Common, public_c::run),
    register(
        "test-tokenizer-1-spm",
        "tests/test-tokenizer-1-spm.cpp",
        Bridge::Common,
        tokenizer_roundtrip::run_spm,
    ),
    register(
        "test-tokenizer-1-bpe",
        "tests/test-tokenizer-1-bpe.cpp",
        Bridge::Common,
        tokenizer_roundtrip::run_bpe,
    ),
    register(
        "test-json-schema-to-grammar",
        "tests/test-json-schema-to-grammar.cpp",
        Bridge::Common,
        json_schema::run,
    ),
    register(
        "test-autorelease",
        "tests/test-autorelease.cpp",
        Bridge::Common,
        |engine| model_lifecycle::run(engine, "tests/test-autorelease.cpp"),
    ),
    register(
        "test-model-load-cancel",
        "tests/test-model-load-cancel.cpp",
        Bridge::Common,
        |engine| model_lifecycle::run(engine, "tests/test-model-load-cancel.cpp"),
    ),
    register(
        "test-save-load-state",
        "tests/test-save-load-state.cpp",
        Bridge::Common,
        |engine| model_lifecycle::run(engine, "tests/test-save-load-state.cpp"),
    ),
    register(
        "test-state-restore-fragmented",
        "tests/test-state-restore-fragmented.cpp",
        Bridge::Common,
        |engine| model_lifecycle::run(engine, "tests/test-state-restore-fragmented.cpp"),
    ),
    register(
        "test-thread-safety",
        "tests/test-thread-safety.cpp",
        Bridge::Common,
        |engine| model_lifecycle::run(engine, "tests/test-thread-safety.cpp"),
    ),
    register(
        "test-chat-auto-parser",
        "tests/test-chat-auto-parser.cpp",
        Bridge::Common,
        chat_auto::run,
    ),
    register(
        "test-tokenizer-0",
        "tests/test-tokenizer-0.cpp",
        Bridge::Common,
        tokenizer::run,
    ),
    register(
        "test-chat-peg-parser",
        "tests/test-chat-peg-parser.cpp",
        Bridge::Common,
        chat_peg::run,
    ),
    register(
        "test-gguf",
        "tests/test-gguf.cpp",
        Bridge::Common,
        gguf::run,
    ),
    register(
        "test-alloc",
        "tests/test-alloc.cpp",
        Bridge::Common,
        allocation::run,
    ),
    register(
        "test-chat-template",
        "tests/test-chat-template.cpp",
        Bridge::Common,
        chat_template::run,
    ),
    register(
        "test-mtmd-c-api",
        "tests/test-mtmd-c-api.c",
        Bridge::Common,
        mtmd_chunks::run,
    ),
    register(
        "test-sampling",
        "tests/test-sampling.cpp",
        Bridge::Common,
        sampling::run,
    ),
    register(
        "peg-basic",
        "tests/peg-parser/test-basic.cpp",
        Bridge::Common,
        peg::run_basic,
    ),
    register(
        "peg-gbnf",
        "tests/peg-parser/test-gbnf-generation.cpp",
        Bridge::Common,
        peg::run_gbnf,
    ),
    register(
        "peg-json",
        "tests/peg-parser/test-json-parser.cpp",
        Bridge::Common,
        peg::run_json,
    ),
    register(
        "peg-serialization",
        "tests/peg-parser/test-json-serialization.cpp",
        Bridge::Common,
        peg::run_serialization,
    ),
    register(
        "peg-python-dict",
        "tests/peg-parser/test-python-dict-parser.cpp",
        Bridge::Common,
        peg::run_python_dict,
    ),
    register(
        "peg-unicode",
        "tests/peg-parser/test-unicode.cpp",
        Bridge::Common,
        peg::run_unicode,
    ),
    register(
        "test-peg-parser",
        "tests/test-peg-parser.cpp",
        Bridge::Common,
        peg::run_all,
    ),
    register(
        "test-batch-alloc",
        "tests/test-batch-alloc.cpp",
        Bridge::Common,
        batch_allocator::run,
    ),
    register(
        "test-pq2-row-shapes",
        "tests/test-pq2-row-shapes.cpp",
        Bridge::Ggml,
        pq2::run,
    ),
    register(
        "test-ptq1_0-element-map",
        "tests/test-ptq1_0-element-map.cpp",
        Bridge::Ggml,
        ptq1::run,
    ),
    register(
        "test-unicode",
        "tests/test-unicode.cpp",
        Bridge::Common,
        common::run_unicode,
    ),
    register(
        "test-mtp-catchup-batch",
        "tests/test-mtp-catchup-batch.cpp",
        Bridge::Common,
        common::run_mtp,
    ),
    register(
        "test-jinja",
        "tests/test-jinja.cpp",
        Bridge::Common,
        jinja::run,
    ),
    register(
        "test-mtmd-impl",
        "tests/test-mtmd-impl.cpp",
        Bridge::Common,
        mtmd::run,
    ),
    register(
        "test-grammar-parser",
        "tests/test-grammar-parser.cpp",
        Bridge::Common,
        grammar_parser::run,
    ),
    register(
        "test-reasoning-budget",
        "tests/test-reasoning-budget.cpp",
        Bridge::Common,
        reasoning_budget::run,
    ),
    register(
        "test-col2im-1d",
        "tests/test-col2im-1d.cpp",
        Bridge::Common,
        numerics::run_col2im,
    ),
    register(
        "test-dfly-fusion",
        "tests/test-dfly-fusion.cpp",
        Bridge::Common,
        numerics::run_dfly,
    ),
    register(
        "test-rope",
        "tests/test-rope.cpp",
        Bridge::Common,
        numerics::run_rope,
    ),
    register(
        "test-llama-grammar",
        "tests/test-llama-grammar.cpp",
        Bridge::Common,
        llama_grammar::run,
    ),
    register(
        "test-grammar-integration",
        "tests/test-grammar-integration.cpp",
        Bridge::Common,
        grammar_integration::run,
    ),
    register(
        "test-quantize-fns",
        "tests/test-quantize-fns.cpp",
        Bridge::Common,
        codec_assertions::run_quantize,
    ),
    register(
        "test-ptq1_0-cuda-dot",
        "tests/test-ptq1_0-cuda-dot.cpp",
        Bridge::Common,
        codec_assertions::run_ptq_cuda,
    ),
    register(
        "test-log",
        "tests/test-log.cpp",
        Bridge::Common,
        logging::run,
    ),
    register(
        "test-moe-weighted-reduce",
        "tests/test-moe-weighted-reduce.cpp",
        Bridge::Common,
        moe_reduce::run,
    ),
];

const BENCHMARKS: &[Case] = &[
    register(
        "backend-ops-performance",
        "tests/test-backend-ops.cpp#performance",
        Bridge::Common,
        backend_ops::benchmark,
    ),
    register(
        "chat-peg",
        "tests/test-chat-peg-parser.cpp#performance",
        Bridge::Common,
        chat_peg::benchmark,
    ),
    register(
        "sampling",
        "tests/test-sampling.cpp#performance",
        Bridge::Common,
        sampling::benchmark,
    ),
    register(
        "peg-deserialize",
        "tests/peg-parser/test-json-serialization.cpp#deserialize",
        Bridge::Common,
        peg::benchmark_deserialization,
    ),
];

fn selected(case: Option<&'static Case>) -> impl Iterator<Item = &'static Case> {
    CASES
        .iter()
        .filter(move |candidate| case.is_none_or(|case| candidate.id == case.id))
}
struct Options {
    target: Target,
    case: Option<&'static Case>,
    benchmark: Option<&'static Case>,
    diagnostic: Option<PathBuf>,
    bridge: Option<PathBuf>,
    common_bridge: Option<PathBuf>,
    evidence_directory: Option<PathBuf>,
    source_directory: Option<PathBuf>,
    asset_directory: Option<PathBuf>,
    output: PathBuf,
}

fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn absolute_path(value: std::ffi::OsString, option: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(invalid(format!("{option} must be an absolute path")));
    }
    Ok(path)
}

fn options() -> Result<Options> {
    let mut target = None;
    let mut case = None;
    let mut benchmark = None;
    let mut diagnostic = None;
    let mut bridge = None;
    let mut common_bridge = None;
    let mut evidence_directory = None;
    let mut source_directory = None;
    let mut asset_directory = None;
    let mut output = None;
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| invalid("every option requires a value"))?;
        match argument.to_str() {
            Some("--target") if target.is_none() => {
                target = Some(match value.to_str() {
                    Some("cpp") => Target::Cpp,
                    Some("rust") => Target::Rust,
                    _ => return Err(invalid("--target must be cpp or rust")),
                });
            }
            Some("--case") if case.is_none() => {
                case = Some(
                    CASES
                        .iter()
                        .find(|case| {
                            value
                                .to_str()
                                .is_some_and(|value| value == case.selector || value == case.id)
                        })
                        .ok_or_else(|| invalid("unknown upstream test case"))?,
                );
            }
            Some("--benchmark") if benchmark.is_none() => {
                benchmark = Some(
                    BENCHMARKS
                        .iter()
                        .find(|case| value.to_str() == Some(case.selector))
                        .ok_or_else(|| invalid("unknown upstream benchmark"))?,
                );
            }
            Some("--bridge") if bridge.is_none() => {
                bridge = Some(absolute_path(value, "--bridge")?);
            }
            Some("--diagnostic") if diagnostic.is_none() => {
                diagnostic = Some(absolute_path(value, "--diagnostic")?);
            }
            Some("--common-bridge") if common_bridge.is_none() => {
                common_bridge = Some(absolute_path(value, "--common-bridge")?);
            }
            Some("--evidence-directory") if evidence_directory.is_none() => {
                evidence_directory = Some(absolute_path(value, "--evidence-directory")?);
            }
            Some("--source-directory") if source_directory.is_none() => {
                source_directory = Some(absolute_path(value, "--source-directory")?);
            }
            Some("--asset-directory") if asset_directory.is_none() => {
                asset_directory = Some(absolute_path(value, "--asset-directory")?);
            }
            Some("--output") if output.is_none() => {
                output = Some(absolute_path(value, "--output")?);
            }
            _ => return Err(invalid("unknown or duplicate upstream suite option")),
        }
    }
    let options = Options {
        target: target.ok_or_else(|| invalid("--target is required"))?,
        case,
        benchmark,
        diagnostic,
        bridge,
        common_bridge,
        evidence_directory,
        source_directory,
        asset_directory,
        output: output.ok_or_else(|| invalid("--output is required"))?,
    };
    options.validate()?;
    Ok(options)
}

impl Options {
    fn validate(&self) -> Result<()> {
        if self.case.is_none()
            && self.benchmark.is_none()
            && self.diagnostic.is_none()
            && (env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
                || env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main"))
        {
            return Err(invalid(
                "The complete source catalog runs only in GitHub Actions on main; select one --case locally.",
            ));
        }
        if (self.benchmark.is_some() || self.diagnostic.is_some())
            && (self.case.is_some()
                || (self.benchmark.is_some() && self.diagnostic.is_some())
                || env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
                || env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main"))
        {
            return Err(invalid(
                "Select one case, CI-only benchmark or CI-only diagnostic on main",
            ));
        }
        if matches!(self.target, Target::Cpp) {
            if self.evidence_directory.is_none() {
                return Err(invalid("--target cpp requires --evidence-directory"));
            }
            if self.benchmark.is_none()
                && self.diagnostic.is_none()
                && selected(self.case).any(|case| case.bridge == Bridge::Ggml)
                && self.bridge.is_none()
            {
                return Err(invalid("selected C++ cases require --bridge"));
            }
            if (self.benchmark.is_some()
                || self.diagnostic.is_some()
                || selected(self.case).any(|case| case.bridge == Bridge::Common))
                && self.common_bridge.is_none()
            {
                return Err(invalid("selected C++ cases require --common-bridge"));
            }
        }

        Ok(())
    }
}

fn result(id: &str, passed: bool, detail: String) -> Value {
    json!({"Id": id, "Status": if passed { "pass" } else { "fail" }, "Detail": Value::String(detail)})
}

fn read_diagnostic(config: &std::path::Path) -> Result<Value> {
    use std::io::Read as _;
    const BACKEND_LIMIT: u64 = 128 * 1024 * 1024;
    let mut bytes = Vec::new();
    fs::File::open(config)?
        .take(BACKEND_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > BACKEND_LIMIT {
        return Err(invalid("diagnostic configuration exceeds size bound"));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let backend = matches!(
        value["Kind"].as_str(),
        Some(
            "backend-plan"
                | "backend-aggregate-plans"
                | "backend-run"
                | "backend-aggregate"
                | "backend-source-aggregate"
                | "backend-options"
                | "backend-benchmark"
        )
    );
    if !backend && bytes.len() > 65536 {
        return Err(invalid("diagnostic configuration exceeds size bound"));
    }
    Ok(value)
}

fn diagnostic(engine: &Engine, config: &std::path::Path) -> Result<Value> {
    let value = read_diagnostic(config)?;
    match value["Kind"].as_str() {
        Some(
            "backend-plan"
            | "backend-aggregate-plans"
            | "backend-run"
            | "backend-aggregate"
            | "backend-source-aggregate"
            | "backend-options"
            | "backend-benchmark",
        ) => backend_ops::diagnostic(engine, &value),
        Some("quant-schema-generate") => {
            llama_grammar::exact_fields(&value, &["Kind", "Confirmed"])?;
            let confirmed = value["Confirmed"]
                .as_bool()
                .ok_or_else(|| invalid("quantization generation confirmation must be boolean"))?;
            quant_selection::generate(
                engine,
                &engine.evidence_path("quantization-generated-schemas")?,
                confirmed,
            )
        }
        Some("architecture-options") => {
            llama_grammar::exact_fields(
                &value,
                &["Kind", "Architecture", "Seed", "Verbose", "Export"],
            )?;
            let architecture = if value["Architecture"].is_null() {
                None
            } else {
                Some(
                    value["Architecture"]
                        .as_str()
                        .ok_or_else(|| invalid("architecture filter must be null or text"))?
                        .to_owned(),
                )
            };
            let output = if value["Export"].as_bool() == Some(false) {
                None
            } else if value["Export"].as_bool() == Some(true) {
                Some(engine.evidence_path("architecture-diagnostic-export")?)
            } else {
                return Err(invalid("architecture export flag must be boolean"));
            };
            let options = architectures::ArchitectureOptions {
                architecture,
                seed: value["Seed"]
                    .as_u64()
                    .ok_or_else(|| invalid("architecture seed must be unsigned"))?,
                verbose: value["Verbose"]
                    .as_bool()
                    .ok_or_else(|| invalid("architecture verbose flag must be boolean"))?,
                output,
            };
            architectures::run_with_options(engine, &options)
        }
        Some("gbnf-validator") => validator::diagnostic(engine, &value),
        Some("dspark-loop-options") => dspark_loop::diagnostic(engine, &value),
        Some("dspark-forward-options") => dspark_forward::diagnostic(engine, &value),
        Some("export-graph-options") => export_graph::diagnostic(engine, &value),
        Some("backend-sampler-options") => backend_sampler::diagnostic(engine, &value),
        Some("dspark-real-options") => {
            llama_grammar::exact_fields(&value, &["Kind", "Options"])?;
            dspark_real::run_with_options(
                engine,
                &serde_json::from_value(value["Options"].clone())?,
            )
        }
        Some("chat-options") => {
            llama_grammar::exact_fields(&value, &["Kind", "Filter", "ForceReconstruction"])?;
            let filter = if value["Filter"].is_null() {
                None
            } else {
                Some(
                    value["Filter"]
                        .as_str()
                        .ok_or_else(|| invalid("chat filter must be null or text"))?,
                )
            };
            let force = value["ForceReconstruction"]
                .as_bool()
                .ok_or_else(|| invalid("chat reconstruction flag must be boolean"))?;
            chat::run_with_options(engine, filter, force)
        }
        Some("tokenizer-export") => tokenizer_diagnostic(engine, &value),
        _ => Err(invalid("unknown diagnostic kind")),
    }
}

fn tokenizer_diagnostic(engine: &Engine, value: &Value) -> Result<Value> {
    let object = value
        .as_object()
        .filter(|object| object.len() == 4)
        .ok_or_else(|| invalid("diagnostic configuration requires four fields"))?;
    let field = |name| {
        object
            .get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("diagnostic field must be text"))
    };
    if field("Kind")? != "tokenizer-export" {
        return Err(invalid("unknown diagnostic kind"));
    }
    tokenizer::export_text(
        engine,
        field("Vocabulary")?,
        std::path::Path::new(field("InputPath")?),
        std::path::Path::new(field("OutputPath")?),
    )
}

fn main() -> Result<()> {
    let options = options()?;
    let engine = Engine::new(
        options.target,
        options.bridge,
        options.common_bridge,
        options.evidence_directory,
        options.source_directory,
        options.asset_directory,
    )?;
    let cases = if let Some(config) = &options.diagnostic {
        vec![diagnostic(&engine, config)?]
    } else if let Some(benchmark) = options.benchmark {
        vec![(benchmark.run)(&engine)?]
    } else {
        selected(options.case)
            .map(|case| (case.run)(&engine))
            .collect::<Result<Vec<_>>>()?
    };
    let report = json!({
        "SchemaVersion": 1,
        "UpstreamCommit": UPSTREAM,
        "Target": match options.target { Target::Cpp => "cpp", Target::Rust => "rust" },
        "Cases": cases,
        "Measure": if options.benchmark.is_some() { "benchmark-workloads" } else if options.diagnostic.is_some() { "diagnostic-workloads" } else { "upstream-source-files" },
    });
    fs::write(options.output, serde_json::to_vec_pretty(&report)?)?;
    // Measured failures belong in the report. Nonzero exits indicate an
    // infrastructure failure, including corrupt inputs or bridge responses.
    Ok(())
}
