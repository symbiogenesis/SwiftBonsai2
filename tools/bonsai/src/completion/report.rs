//! Legacy CPU-check report fields and admission decisions, without model execution.

use serde_json::{Map, Value, json};

const FIELDS: &[&str] = &[
    "SchemaVersion",
    "RecordedAtUtc",
    "Profile",
    "Status",
    "AvailablePhysicalBytesAtAdmission",
    "RequiredAvailableBytes",
    "ServiceCeilingBytes",
    "AvailableMemoryFloorBytes",
    "TokenLimit",
    "TimeoutSeconds",
    "ContextTokens",
    "BatchTokens",
    "MicrobatchTokens",
    "EstimatedWeightBytes",
    "EstimatedKvBytes",
    "EstimatedRecurrentBytes",
    "EstimatedTransformBytes",
    "EstimatedAdapterBytes",
    "AdapterRequested",
    "WeightRepresentation",
    "ArtifactSha256",
    "EstimatedTransformConstructionTemporaryBytes",
    "EstimatedKnownSubtotalBytes",
    "EstimatedRuntimeHeadroomBytes",
    "EstimateScope",
    "ProcessOwnership",
    "JobCommittedMemoryLimitBytes",
    "ActiveProcessLimit",
    "BreakawayAllowed",
    "ModelRevision",
    "RuntimeCommit",
    "ModelPath",
    "AdapterPath",
    "EffectiveArguments",
    "ModelProcessStarted",
    "Qualification",
];

pub(super) fn attach(plan: &mut Value, available: u64) -> crate::Result<()> {
    let policy = &plan["Policy"];
    let required = policy["RequiredAvailableBytes"]
        .as_u64()
        .ok_or("Missing CPU admission threshold.")?;
    let admitted = available >= required;
    let adapter = !plan["AdapterLock"].is_null();
    let mut report = Map::new();
    for &name in FIELDS {
        let value = match name {
            "SchemaVersion" | "ActiveProcessLimit" => json!(1),
            // PowerShell stamps its existing round-trip UTC string after the CLI returns.
            "RecordedAtUtc" => Value::Null,
            "Profile" => policy["Name"].clone(),
            "Status" => json!(if admitted {
                "admission-passed"
            } else {
                "not-run-insufficient-available-memory"
            }),
            "AvailablePhysicalBytesAtAdmission" => json!(available),
            "AdapterRequested" => json!(adapter),
            "ArtifactSha256" => plan["Artifact"]["Sha256"].clone(),
            "ProcessOwnership" => json!("windows-job-at-creation-kill-on-close"),
            "JobCommittedMemoryLimitBytes" => policy["ServiceCeilingBytes"].clone(),
            "BreakawayAllowed" | "ModelProcessStarted" => json!(false),
            "ModelRevision" => plan["Lock"]["model"]["revision"].clone(),
            "RuntimeCommit" => plan["Lock"]["runtime"]["commit"].clone(),
            "ModelPath" | "AdapterPath" => plan[name].clone(),
            "EffectiveArguments" => plan["Arguments"].clone(),
            "Qualification" => {
                json!("trusted-input-development-only; no daily security or performance acceptance")
            }
            _ => policy
                .get(name)
                .ok_or("Missing CPU report policy field.")?
                .clone(),
        };
        report.insert(name.to_owned(), value);
    }
    let mut order = FIELDS.to_vec();
    if adapter {
        let pin = &plan["AdapterLock"]["adapter"];
        for (name, value) in [
            ("AdapterRevision", pin["revision"].clone()),
            ("AdapterSha256", pin["sha256"].clone()),
            ("AdapterScale", json!(1.0)),
            (
                "AdapterQualification",
                json!(
                    "experimental; native loading/decode is not behavioral, creative quality or performance acceptance"
                ),
            ),
        ] {
            report.insert(name.into(), value);
            order.push(name);
        }
    }
    if !admitted {
        report.insert("Action".into(), json!("Free memory by closing applications you choose, then rerun. This check will not close them."));
        order.push("Action");
    }
    plan["Report"] = Value::Object(report);
    plan["ReportFieldOrder"] = json!(order);
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn frozen_reports_preserve_admission_fields_with_reviewed_shorter_deadline() -> crate::Result<()>
    {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/cpu-reports-v1.json"))
                .map_err(|error| error.to_string())?;
        for case in fixture["cases"]
            .as_array()
            .ok_or("Missing frozen reports.")?
        {
            let mut plan = bonsai_cpu_policy::run(case["request"].clone())?;
            attach(
                &mut plan,
                case["availableBytes"]
                    .as_u64()
                    .ok_or("Invalid memory fixture.")?,
            )?;
            plan["Report"]["RecordedAtUtc"] = fixture["recordedAtUtc"].clone();
            let mut expected = case["expected"].clone();
            // Keep the historical capture intact while enforcing the new device budget.
            assert_eq!(expected["TimeoutSeconds"], 600);
            expected["TimeoutSeconds"] = json!(180);
            assert_eq!(plan["Report"], expected);
            assert_eq!(plan["ReportFieldOrder"], case["fieldOrder"]);
        }
        Ok(())
    }

    #[test]
    fn swift_admission_checks_the_exact_reduced_boundary_for_every_artifact() -> crate::Result<()> {
        for use_adapter in [false, true] {
            for use_ptq1 in [false, true] {
                let original = bonsai_cpu_policy::run(json!({
                    "operation": "plan",
                    "dataDirectory": std::env::temp_dir().join("Bonsai admission fixture"),
                    "profile": "swift-pq2-assessment",
                    "useAbliterateLora": use_adapter,
                    "useRepackedPtq1": use_ptq1,
                }))?;
                for (available, admitted) in [
                    (16_642_998_271_u64, false),
                    (16_642_998_272, true),
                    (16_911_433_728, true),
                    (17_179_869_184, true),
                ] {
                    let mut plan = original.clone();
                    attach(&mut plan, available)?;
                    let report = &plan["Report"];
                    assert_eq!(report["AvailablePhysicalBytesAtAdmission"], available);
                    assert_eq!(report["RequiredAvailableBytes"], 16_642_998_272_u64);
                    assert_eq!(report["JobCommittedMemoryLimitBytes"], 12_348_030_976_u64);
                    assert_eq!(report["AvailableMemoryFloorBytes"], 4_294_967_296_u64);
                    assert_eq!(report["ModelProcessStarted"], false);
                    assert_eq!(
                        report["Status"],
                        if admitted {
                            "admission-passed"
                        } else {
                            "not-run-insufficient-available-memory"
                        }
                    );
                    assert_eq!(report.get("Action").is_none(), admitted);
                }
            }
        }
        Ok(())
    }
}
