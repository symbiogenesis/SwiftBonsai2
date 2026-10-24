//! Safe Rust ownership of assertions from pinned, MIT-licensed Prism tests.
//!
//! Both targets run the same Rust inputs, reference calculations and assertions.
//! The C++ target delegates production operations to a separate native bridge.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Sibling modules share adapters explicitly; unreachable_pub rejects unrestricted pub."
)]

#[path = "upstream_prism/common.rs"]
mod common;
#[path = "upstream_prism/engine.rs"]
mod engine;
#[path = "upstream_prism/fixture.rs"]
mod fixture;
#[path = "upstream_prism/grammar_parser.rs"]
mod grammar_parser;
#[path = "upstream_prism/jinja.rs"]
mod jinja;
#[path = "upstream_prism/mtmd.rs"]
mod mtmd;
#[path = "upstream_prism/pq2.rs"]
mod pq2;
#[path = "upstream_prism/ptq1.rs"]
mod ptq1;
#[path = "upstream_prism/reasoning_budget.rs"]
mod reasoning_budget;

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
const JINJA_ID: &str = "tests/test-jinja.cpp";
const MTMD_ID: &str = "tests/test-mtmd-impl.cpp";
const GRAMMAR_PARSER_ID: &str = "tests/test-grammar-parser.cpp";
const REASONING_BUDGET_ID: &str = "tests/test-reasoning-budget.cpp";

struct Options {
    target: Target,
    case: Option<String>,
    bridge: Option<PathBuf>,
    common_bridge: Option<PathBuf>,
    evidence_directory: Option<PathBuf>,
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
    let mut bridge = None;
    let mut common_bridge = None;
    let mut evidence_directory = None;
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
                case = Some(match value.to_str() {
                    Some("test-pq2-row-shapes" | PQ2_ID) => PQ2_ID.to_owned(),
                    Some("test-ptq1_0-element-map" | PTQ1_ID) => PTQ1_ID.to_owned(),
                    Some("test-unicode" | UNICODE_ID) => UNICODE_ID.to_owned(),
                    Some("test-mtp-catchup-batch" | MTP_ID) => MTP_ID.to_owned(),
                    Some("test-jinja" | JINJA_ID) => JINJA_ID.to_owned(),
                    Some("test-mtmd-impl" | MTMD_ID) => MTMD_ID.to_owned(),
                    Some("test-grammar-parser" | GRAMMAR_PARSER_ID) => GRAMMAR_PARSER_ID.to_owned(),
                    Some("test-reasoning-budget" | REASONING_BUDGET_ID) => {
                        REASONING_BUDGET_ID.to_owned()
                    }
                    _ => return Err(invalid("unknown upstream test case")),
                });
            }
            Some("--bridge") if bridge.is_none() => {
                bridge = Some(absolute_path(value, "--bridge")?);
            }
            Some("--common-bridge") if common_bridge.is_none() => {
                common_bridge = Some(absolute_path(value, "--common-bridge")?);
            }
            Some("--evidence-directory") if evidence_directory.is_none() => {
                evidence_directory = Some(absolute_path(value, "--evidence-directory")?);
            }
            Some("--output") if output.is_none() => {
                output = Some(absolute_path(value, "--output")?);
            }
            _ => return Err(invalid("unknown or duplicate upstream suite option")),
        }
    }
    let target = target.ok_or_else(|| invalid("--target is required"))?;
    if matches!(target, Target::Cpp) {
        if evidence_directory.is_none() {
            return Err(invalid("--target cpp requires --evidence-directory"));
        }
        if case
            .as_deref()
            .is_none_or(|case| matches!(case, PQ2_ID | PTQ1_ID))
            && bridge.is_none()
        {
            return Err(invalid("selected C++ cases require --bridge"));
        }
        if case.as_deref().is_none_or(|case| {
            matches!(
                case,
                UNICODE_ID | MTP_ID | JINJA_ID | MTMD_ID | GRAMMAR_PARSER_ID | REASONING_BUDGET_ID
            )
        }) && common_bridge.is_none()
        {
            return Err(invalid("selected C++ cases require --common-bridge"));
        }
    }
    Ok(Options {
        target,
        case,
        bridge,
        common_bridge,
        evidence_directory,
        output: output.ok_or_else(|| invalid("--output is required"))?,
    })
}

fn result(id: &str, passed: bool, detail: String) -> Value {
    json!({"Id": id, "Status": if passed { "pass" } else { "fail" }, "Detail": Value::String(detail)})
}

fn main() -> Result<()> {
    let options = options()?;
    let engine = Engine::new(
        options.target,
        options.bridge,
        options.common_bridge,
        options.evidence_directory,
    )?;
    let mut cases = Vec::new();
    if options.case.as_deref().is_none_or(|case| case == PQ2_ID) {
        cases.push(pq2::run(&engine)?);
    }
    if options.case.as_deref().is_none_or(|case| case == PTQ1_ID) {
        cases.push(ptq1::run(&engine)?);
    }
    if options
        .case
        .as_deref()
        .is_none_or(|case| case == UNICODE_ID)
    {
        cases.push(common::run_unicode(&engine)?);
    }
    if options.case.as_deref().is_none_or(|case| case == MTP_ID) {
        cases.push(common::run_mtp(&engine)?);
    }
    if options.case.as_deref().is_none_or(|case| case == JINJA_ID) {
        cases.push(jinja::run(&engine)?);
    }
    if options.case.as_deref().is_none_or(|case| case == MTMD_ID) {
        cases.push(mtmd::run(&engine)?);
    }
    if options
        .case
        .as_deref()
        .is_none_or(|case| case == GRAMMAR_PARSER_ID)
    {
        cases.push(grammar_parser::run(&engine)?);
    }
    if options
        .case
        .as_deref()
        .is_none_or(|case| case == REASONING_BUDGET_ID)
    {
        cases.push(reasoning_budget::run(&engine)?);
    }
    let report = json!({
        "SchemaVersion": 1,
        "UpstreamCommit": UPSTREAM,
        "Target": match options.target { Target::Cpp => "cpp", Target::Rust => "rust" },
        "Cases": cases,
    });
    fs::write(options.output, serde_json::to_vec_pretty(&report)?)?;
    // Measured failures belong in the report. Nonzero exits indicate an
    // infrastructure failure, including corrupt inputs or bridge responses.
    Ok(())
}
