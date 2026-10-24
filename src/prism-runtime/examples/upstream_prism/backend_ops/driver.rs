// SPDX-License-Identifier: MIT
//! Deterministic case execution and complete partition/source accounting.

use super::super::engine::Engine;
use super::super::{Result, invalid};
use super::evaluate;
use super::gradient;
use super::graph::Plan;
use super::host::Production;
use super::performance;
use super::planner;
use super::prepare::{Mode, prepare};
use super::program::{Geometry, SOURCE_SHA256};
use super::transport;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn successful(status: &Json, mode: Mode) -> bool {
    matches!(status.as_str(), Some("pass" | "skip")) || (mode == Mode::Perf && status == "measured")
}

fn cleanup(directory: &Path) -> Result<()> {
    let owned = fs::canonicalize(directory)?;
    if !owned.is_absolute() {
        return Err(invalid("backend owned cleanup directory is not absolute"));
    }
    for entry in fs::read_dir(&owned)? {
        let entry = entry?;
        if entry.file_name() == "case-receipt.json" {
            continue;
        }
        let path = fs::canonicalize(entry.path())?;
        if !path.starts_with(&owned) || !entry.file_type()?.is_file() {
            return Err(invalid(
                "backend cleanup encountered an unowned or non-file artifact",
            ));
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

fn one(
    engine: &Engine,
    index: usize,
    geometry: &Geometry,
    mode: Mode,
    row: &Json,
    plan: &Json,
    directory: &Path,
) -> Result<Option<Json>> {
    let (program, _, _) = planner::extent(mode)?;
    let threads = usize::try_from(
        plan["Threads"]
            .as_u64()
            .ok_or_else(|| invalid("backend plan thread count missing"))?,
    )?;
    if !(1..=256).contains(&threads) {
        return Err(invalid("backend source thread count exceeds native bound"));
    }
    let filter = plan["Filter"].as_str();
    let offset = row["CrtOffset"]
        .as_u64()
        .ok_or_else(|| invalid("backend planned CRT offset missing"))?;
    transport::with_session(engine, directory, |session| {
        let mut host = Production::new(
            Plan::new(session),
            program,
            index as u64,
            threads,
            mode.code(),
        );
        let mut prepared = prepare(&mut host, geometry, mode, filter)?;
        let observed = planner::receipt(&host, &prepared, geometry, index)?;
        for name in [
            "Index",
            "Class",
            "ArgumentsSha256",
            "ShouldInitialize",
            "CrtDraws",
            "Supported",
            "SkipReason",
            "Operation",
            "Variables",
            "ReferenceAvailable",
            "MetadataSha256",
        ] {
            if observed[name] != row[name] {
                return Err(invalid(format!(
                    "backend execution preparation differs from source plan at {name}"
                )));
            }
        }
        let mut result = if prepared.initialize {
            evaluate::allocate(&mut host, &mut prepared, geometry, mode, offset)?;
            match mode {
                Mode::Eval => evaluate::evaluate(&mut host, &mut prepared, geometry)?,
                Mode::Grad => gradient::evaluate(&mut host, &mut prepared, geometry)?,
                Mode::Perf => performance::evaluate(&mut host, &mut prepared, geometry)?,
                Mode::Support => {
                    return Err(invalid("source support mode cannot initialize tensors"));
                }
            }
        } else {
            json!({"Status":if prepared.skip.is_some(){"skip"}else{"pass"},"Supported":prepared.supported,"SkipReason":prepared.skip})
        };
        result["Index"] = json!(index);
        result["Class"] = json!(geometry.class);
        result["ArgumentsSha256"] = json!(planner::arguments_hash(geometry)?);
        result["Operation"] = json!(prepared.operation);
        result["Variables"] = json!(prepared.variables);
        result["CrtOffset"] = json!(offset);
        result["CrtDraws"] = json!(prepared.crt_draws);
        result["Supported"] = json!(prepared.supported);
        result["SourceAssertions"] = json!(host.assertions);
        result["RandomDeviceSeeds"] = json!(host.inputs.ledger.seeds);
        result["InputAndOutputReceipts"] = json!(host.inputs.receipts);
        Ok(result)
    })
}

pub(super) fn run(
    engine: &Engine,
    index: usize,
    count: usize,
    mode: Mode,
    plan: &Json,
) -> Result<Json> {
    if mode == Mode::Perf {
        performance::require_ci()?;
    }
    planner::partition(index, count)?;
    let rows = planner::validate(plan, mode)?;
    let (_, geometries, geometry_hash) = planner::extent(mode)?;
    let mut items = Vec::new();
    for (case_index, geometry) in geometries
        .iter()
        .enumerate()
        .filter(|(case_index, _)| case_index % count == index)
    {
        let directory =
            engine.evidence_path(&format!("backend-{}-run-{case_index:05}", mode.name()))?;
        let observed = one(
            engine,
            case_index,
            geometry,
            mode,
            &rows[case_index],
            plan,
            &directory,
        );
        let observed = match observed {
            Ok(Some(value)) => value,
            Ok(None) => {
                return Ok(
                    json!({"SchemaVersion":1,"SourceId":"test-backend-ops","SourceSha256":SOURCE_SHA256,"GeometrySha256":geometry_hash,"Mode":mode.name(),"CpuSelection":"explicit CPU","Partition":{"Index":index,"Count":count},"Status":"unsupported","Reason":"unsupported production API: backend graph execution, allocation and comparison","PlanSha256":plan["PlanSha256"],"ExpectedGeometries":geometries.len(),"Items":items}),
                );
            }
            Err(error) => {
                json!({"Index":case_index,"Class":geometry.class,"ArgumentsSha256":planner::arguments_hash(geometry)?,"Status":"error","Reason":error.to_string(),"CrtOffset":rows[case_index]["CrtOffset"],"CrtDraws":rows[case_index]["CrtDraws"]})
            }
        };
        fs::write(
            directory.join("case-receipt.json"),
            serde_json::to_vec_pretty(&observed)?,
        )?;
        if matches!(
            observed["Status"].as_str(),
            Some("pass" | "skip" | "measured")
        ) {
            cleanup(&directory)?;
        }
        items.push(observed);
    }
    let passed = items.iter().all(|item| successful(&item["Status"], mode));
    Ok(
        json!({"SchemaVersion":1,"SourceId":"test-backend-ops","SourceSha256":SOURCE_SHA256,"GeometrySha256":geometry_hash,"Mode":mode.name(),"CpuSelection":"explicit CPU","Partition":{"Index":index,"Count":count},"Status":if passed{"pass"}else{"fail"},"PlanSha256":plan["PlanSha256"],"ExpectedGeometries":geometries.len(),"Items":items}),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "The complete source-extent proof checks every partition and ordered geometry against the committed plan in one transaction."
)]
pub(super) fn aggregate(reports: &[Json], plan: &Json) -> Result<Json> {
    let mode = Mode::parse(
        plan["Mode"]
            .as_str()
            .ok_or_else(|| invalid("backend aggregate mode missing"))?,
    )?;
    let rows = planner::validate(plan, mode)?;
    let (_, geometries, geometry_hash) = planner::extent(mode)?;
    let first = reports
        .first()
        .ok_or_else(|| invalid("backend result aggregation has no partitions"))?;
    let count = usize::try_from(
        first["Partition"]["Count"]
            .as_u64()
            .ok_or_else(|| invalid("backend result partition count missing"))?,
    )?;
    if reports.len() != count {
        return Err(invalid("backend result partition count incomplete"));
    }
    let mut partitions = BTreeSet::new();
    let mut items = BTreeMap::new();
    let mut passed = true;
    for report in reports {
        if report["SchemaVersion"] != 1
            || report["SourceId"] != "test-backend-ops"
            || report["SourceSha256"] != SOURCE_SHA256
            || report["GeometrySha256"] != geometry_hash
            || report["Mode"] != mode.name()
            || report["CpuSelection"] != "explicit CPU"
            || report["PlanSha256"] != plan["PlanSha256"]
            || report["ExpectedGeometries"] != geometries.len()
            || report["Partition"]["Count"] != count
            || !matches!(report["Status"].as_str(), Some("pass" | "fail"))
        {
            return Err(invalid(
                "backend result provenance, plan or production completeness differs",
            ));
        }
        passed &= report["Status"] == "pass";
        let index = usize::try_from(
            report["Partition"]["Index"]
                .as_u64()
                .ok_or_else(|| invalid("backend result partition index missing"))?,
        )?;
        planner::partition(index, count)?;
        if !partitions.insert(index) {
            return Err(invalid("duplicate backend result partition"));
        }
        let selected = report["Items"]
            .as_array()
            .ok_or_else(|| invalid("backend result rows missing"))?;
        let expected = geometries
            .iter()
            .enumerate()
            .filter(|(case_index, _)| case_index % count == index)
            .map(|(case_index, _)| case_index)
            .collect::<Vec<_>>();
        if selected.len() != expected.len() {
            return Err(invalid(
                "backend result partition geometry extent incomplete",
            ));
        }
        for (row, &expected_index) in selected.iter().zip(&expected) {
            if row["Index"] != expected_index
                || row["Class"] != geometries[expected_index].class
                || row["ArgumentsSha256"] != rows[expected_index]["ArgumentsSha256"]
                || row["CrtOffset"] != rows[expected_index]["CrtOffset"]
                || row["CrtDraws"] != rows[expected_index]["CrtDraws"]
                || !matches!(
                    row["Status"].as_str(),
                    Some("pass" | "skip" | "measured" | "fail" | "error")
                )
            {
                return Err(invalid(
                    "backend result geometry identity/order or input-stream receipt differs",
                ));
            }
            if row["Status"] == "measured" && mode != Mode::Perf {
                return Err(invalid(
                    "performance measurement cannot satisfy a correctness geometry",
                ));
            }
            passed &= successful(&row["Status"], mode);
            if items.insert(expected_index, row.clone()).is_some() {
                return Err(invalid("duplicate backend result geometry"));
            }
        }
    }
    if items.len() != geometries.len() {
        return Err(invalid(
            "backend complete source result geometry extent missing",
        ));
    }
    let mut pass_count = 0;
    let mut skip_count = 0;
    let mut fail_count = 0;
    let mut error_count = 0;
    let mut digests = Vec::with_capacity(items.len());
    for (index, row) in items {
        if index != digests.len() {
            return Err(invalid("backend result ordered geometry gap"));
        }
        match row["Status"].as_str() {
            Some("pass") => pass_count += 1,
            Some("measured") if mode == Mode::Perf => pass_count += 1,
            Some("skip") => skip_count += 1,
            Some("fail") => fail_count += 1,
            Some("error") => error_count += 1,
            _ => return Err(invalid("backend result status unknown")),
        }
        digests.push(planner::hash(&serde_json::to_vec(&row)?));
    }
    Ok(
        json!({"SchemaVersion":1,"SourceId":"test-backend-ops","SourceSha256":SOURCE_SHA256,"GeometrySha256":geometry_hash,"Mode":mode.name(),"CpuSelection":"explicit CPU","Status":if passed{"pass"}else{"fail"},"PlanSha256":plan["PlanSha256"],"ExpectedGeometries":geometries.len(),"CompletedGeometries":digests.len(),"PassedGeometries":pass_count,"OriginalSkippedGeometries":skip_count,"FailedGeometries":fail_count,"ErrorGeometries":error_count,"OrderedResultSha256":planner::hash(&serde_json::to_vec(&digests)?),"TotalCrtDraws":plan["TotalCrtDraws"]}),
    )
}
