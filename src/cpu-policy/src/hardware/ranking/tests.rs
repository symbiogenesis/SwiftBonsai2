//! Ranking is independent of marketing family and never opens a device.

use super::*;
use serde_json::json;

struct Fixture {
    key: ProfileCompatibilityKey,
    evidence: ProfileEvidence,
    comparison: String,
}

impl Fixture {
    fn new(objective: Objective) -> Result<Self, String> {
        let key = ProfileCompatibilityKey::from_json(
            &json!({
                "schema_version": 1, "objective": objective,
                "model_sha256": "a".repeat(64), "adapter": {"kind": "none"},
                "runtime": {"archive_sha256": "b".repeat(64),
                    "source_revision": "c".repeat(40),
                    "target": {"architecture": "aarch64", "os": "windows", "environment": "msvc"}},
                "capability_class_sha256": "d".repeat(64),
                "backend_driver_sha256": "e".repeat(64),
                "graph_contract": "swift-pq2", "state_layout": "hybrid-fp32-fp16",
                "settings_sha256": "f".repeat(64),
                "workload": {"context_tokens": 8192, "minimum_prompt_tokens": 1,
                    "maximum_prompt_tokens": 4096, "maximum_output_tokens": 4096, "state": "fresh"}
            })
            .to_string(),
        )?;
        let evidence = ProfileEvidence::from_json(
            &json!({
                "schema_version": 1, "compatibility": key,
                "tested_main_revision": "1".repeat(40), "ci_run_id": "123",
                "capture_sha256": "2".repeat(64), "verdict": "passed", "energy_verdict": "passed"
            })
            .to_string(),
        )?;
        Ok(Self {
            key,
            evidence,
            comparison: "3".repeat(64),
        })
    }

    fn observation(&self, id: &'static str, backend: Backend, latency: u64) -> Observation<'_> {
        Observation {
            id,
            current: &self.key,
            evidence: &self.evidence,
            independently_reviewed: true,
            ready_and_admitted: true,
            placement: Placement {
                prefill: backend,
                decode: backend,
            },
            phase_handoff_reviewed: false,
            comparison_sha256: &self.comparison,
            complete_task_ns: latency,
            upper_task_ns: latency,
            energy_uj: Some(100),
            upper_energy_uj: Some(100),
        }
    }
}

#[test]
fn relative_cpu_gpu_npu_costs_choose_each_winner() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let gpu = fixture.observation("gpu", Backend::Adreno, 80);
    let mut npu = fixture.observation("npu", Backend::Hexagon, 120);
    assert_eq!(recommend(&cpu, &[gpu, npu])?.profile_id, "gpu");
    npu.complete_task_ns = 70;
    npu.upper_task_ns = 75;
    assert_eq!(recommend(&cpu, &[gpu, npu])?.profile_id, "npu");
    assert_eq!(recommend(&cpu, &[npu, gpu])?.profile_id, "npu");
    npu.complete_task_ns = 95;
    npu.upper_task_ns = 96;
    assert_eq!(recommend(&cpu, &[npu])?.profile_id, "cpu");
    Ok(())
}

#[test]
fn evidence_assertions_and_stale_driver_never_admit_npu() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let npu = fixture.observation("npu", Backend::Hexagon, 50);
    for candidate in [
        Observation {
            independently_reviewed: false,
            ..npu
        },
        Observation {
            ready_and_admitted: false,
            ..npu
        },
    ] {
        assert_eq!(recommend(&cpu, &[candidate])?.profile_id, "cpu");
    }
    let mut changed = fixture.key.clone();
    changed.0.backend_driver_sha256 = "9".repeat(64);
    assert_eq!(
        recommend(
            &cpu,
            &[Observation {
                current: &changed,
                ..npu
            }]
        )?
        .profile_id,
        "cpu"
    );
    let mut losing = fixture.evidence.clone();
    for verdict in [Verdict::Unknown, Verdict::Failed, Verdict::NoGo] {
        losing.0.verdict = verdict;
        assert_eq!(
            recommend(
                &cpu,
                &[Observation {
                    evidence: &losing,
                    ..npu
                }]
            )?
            .profile_id,
            "cpu"
        );
    }
    Ok(())
}

#[test]
fn comparison_must_preserve_model_device_workload_and_settings() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let npu = fixture.observation("npu", Backend::Hexagon, 50);
    let other_comparison = "4".repeat(64);
    assert_eq!(
        recommend(
            &cpu,
            &[Observation {
                comparison_sha256: &other_comparison,
                ..npu
            }]
        )?
        .profile_id,
        "cpu"
    );
    for change in 0..4 {
        let mut other = Fixture::new(Objective::Interactive)?;
        match change {
            0 => other.key.0.model_sha256 = "9".repeat(64),
            1 => other.key.0.capability_class_sha256 = "9".repeat(64),
            2 => other.key.0.workload.maximum_output_tokens = 2048,
            _ => other.key.0.workload.state = super::super::StateUse::Reused,
        }
        other.evidence.0.compatibility = other.key.0.clone();
        assert_eq!(
            recommend(&cpu, &[other.observation("other", Backend::Hexagon, 50)])?.profile_id,
            "cpu"
        );
    }
    Ok(())
}

#[test]
fn uncertainty_threshold_and_ties_are_deterministic_without_overflow() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let mut npu = fixture.observation("npu", Backend::Hexagon, 70);
    npu.upper_task_ns = 91;
    assert_eq!(recommend(&cpu, &[npu])?.profile_id, "cpu");
    npu.upper_task_ns = 90;
    assert_eq!(recommend(&cpu, &[npu])?.profile_id, "npu");
    let gpu = fixture.observation("gpu", Backend::Adreno, 70);
    assert_eq!(recommend(&cpu, &[npu, gpu])?.profile_id, "gpu");
    assert_eq!(recommend(&cpu, &[gpu, npu])?.profile_id, "gpu");
    assert!(!improves_by_ten_percent(u64::MAX, u64::MAX));
    Ok(())
}

#[test]
fn mixed_phases_require_whole_request_handoff_review() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let mut mixed = fixture.observation("mixed", Backend::Hexagon, 70);
    mixed.placement.decode = Backend::Cpu;
    assert_eq!(recommend(&cpu, &[mixed])?.profile_id, "cpu");
    mixed.phase_handoff_reviewed = true;
    assert_eq!(recommend(&cpu, &[mixed])?.placement, mixed.placement);
    let gpu = fixture.observation("gpu", Backend::Adreno, 70);
    assert_eq!(recommend(&cpu, &[gpu, mixed])?.profile_id, "mixed");
    Ok(())
}

#[test]
fn distinct_exact_build_and_driver_receipts_can_compete() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let mut other = Fixture::new(Objective::Interactive)?;
    other.key.0.runtime.archive_sha256 = "5".repeat(64);
    other.key.0.runtime.source_revision = "6".repeat(40);
    other.key.0.backend_driver_sha256 = "7".repeat(64);
    other.key.0.settings_sha256 = "8".repeat(64);
    other.evidence.0.compatibility = other.key.0.clone();
    assert_eq!(
        recommend(&cpu, &[other.observation("npu", Backend::Hexagon, 70)])?.profile_id,
        "npu"
    );
    Ok(())
}

#[test]
fn battery_needs_energy_evidence_and_bounded_latency() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Battery)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let npu = fixture.observation("npu", Backend::Hexagon, 80);
    let efficient = Observation {
        energy_uj: Some(70),
        upper_energy_uj: Some(80),
        ..npu
    };
    assert_eq!(recommend(&cpu, &[npu])?.profile_id, "cpu");
    assert_eq!(recommend(&cpu, &[efficient])?.profile_id, "npu");
    for candidate in [
        Observation {
            energy_uj: None,
            upper_energy_uj: None,
            ..efficient
        },
        Observation {
            upper_task_ns: 101,
            ..efficient
        },
    ] {
        assert_eq!(recommend(&cpu, &[candidate])?.profile_id, "cpu");
    }
    assert!(
        recommend(
            &Observation {
                energy_uj: None,
                ..cpu
            },
            &[efficient]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn missing_control_and_duplicate_ids_reject() -> Result<(), String> {
    let fixture = Fixture::new(Objective::Interactive)?;
    let cpu = fixture.observation("cpu", Backend::Cpu, 100);
    let npu = fixture.observation("npu", Backend::Hexagon, 70);
    assert!(
        recommend(
            &Observation {
                ready_and_admitted: false,
                ..cpu
            },
            &[npu]
        )
        .is_err()
    );
    assert!(recommend(&npu, &[]).is_err());
    assert!(recommend(&cpu, &[npu, npu]).is_err());
    assert!(recommend(&cpu, &[cpu]).is_err());
    assert!(
        recommend(
            &cpu,
            &[Observation {
                complete_task_ns: 0,
                ..npu
            }]
        )?
        .placement
        .cpu_only()
    );
    Ok(())
}
