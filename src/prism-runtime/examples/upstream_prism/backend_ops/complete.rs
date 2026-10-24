// SPDX-License-Identifier: MIT
//! A single backend source verdict requires every correctness mode and every source geometry.

use super::super::engine::Engine;
use super::super::{Result, invalid, result};
use super::{driver, planner, prepare::Mode, program};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

pub(super) fn aggregate(modes: &[Json]) -> Result<Json> {
    if modes.len() != 3 {
        return Err(invalid(
            "backend source requires complete eval, grad and support mode receipts",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut passed = true;
    let mut receipts = Vec::new();
    for mode in modes {
        let name = mode["Mode"]
            .as_str()
            .ok_or_else(|| invalid("backend mode result name missing"))?;
        if !matches!(name, "eval" | "grad" | "support") || !seen.insert(name) {
            return Err(invalid(
                "backend source correctness modes are missing or duplicated",
            ));
        }
        if mode["SchemaVersion"] != 1
            || mode["SourceId"] != "test-backend-ops"
            || mode["SourceSha256"] != program::SOURCE_SHA256
            || mode["GeometrySha256"]
                != "1724d4a88170deb030f676d81f97ce050ab6ee32c37452626fd834ea1c4f7419"
            || mode["CpuSelection"] != "explicit CPU"
            || mode["ExpectedGeometries"] != program::EVAL_COUNT
            || mode["CompletedGeometries"] != program::EVAL_COUNT
            || !matches!(mode["Status"].as_str(), Some("pass" | "fail"))
        {
            return Err(invalid(
                "backend source mode provenance/completeness differs",
            ));
        }
        let count = [
            "PassedGeometries",
            "OriginalSkippedGeometries",
            "FailedGeometries",
            "ErrorGeometries",
        ]
        .into_iter()
        .try_fold(0_u64, |count, name| {
            mode[name]
                .as_u64()
                .and_then(|value| count.checked_add(value))
                .ok_or_else(|| invalid("backend mode geometry counts missing or overflow"))
        })?;
        if count != program::EVAL_COUNT as u64 {
            return Err(invalid(
                "backend source mode counts do not cover its complete matrix",
            ));
        }
        passed &= mode["Status"] == "pass";
        receipts.push(mode.clone());
    }
    let mut source = result(
        "test-backend-ops",
        passed,
        format!(
            "CPU explicitly selected; all {} ordered geometries accounted for in eval, grad and support; benchmark1530 workloads are separate CI-only measurements",
            program::EVAL_COUNT
        ),
    );
    source["SourceSha256"] = json!(program::SOURCE_SHA256);
    source["Modes"] = json!(receipts);
    source["CorrectnessModeCount"] = json!(3);
    source["SourceCount"] = json!(1);
    source["MetalForcedFaSlice"] = json!({"Status":"original skip","Reason":"CPU backend has no Metal tuning procedures; Apple platforms are outside this target"});
    Ok(source)
}

pub(super) fn run(engine: &Engine) -> Result<Json> {
    let mut modes = Vec::new();
    for mode in [Mode::Eval, Mode::Grad, Mode::Support] {
        let planned = planner::plan(engine, 0, 1, mode, None)?;
        if planned["Status"] != "planned" {
            let mut unsupported=result("test-backend-ops",false,"unsupported production API: generic backend graph/support/allocation/comparison; no replacement Rust replica is used".into());
            unsupported["Coverage"] = planned;
            return Ok(unsupported);
        }
        let plan = planner::aggregate(&[planned])?;
        let executed = driver::run(engine, 0, 1, mode, &plan)?;
        modes.push(driver::aggregate(&[executed], &plan)?);
    }
    aggregate(&modes)
}
