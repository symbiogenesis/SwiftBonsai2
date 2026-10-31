//! Deterministic comparison policy, without execution or admission authority.
//!
//! Callers must independently verify archived evidence, signatures, effective
//! placement and live resource admission. This module neither trusts receipt
//! assertions by themselves nor installs a profile in the serving application.

use super::{Objective, ProfileCompatibilityKey, ProfileEvidence, Verdict};

/// Execution unit used by one phase of a measured complete-request profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    /// Qualified scalar/NEON CPU execution.
    Cpu,
    /// Qualified Adreno execution.
    Adreno,
    /// Qualified Hexagon execution.
    Hexagon,
}

/// A measured plan; mixed placement requires reviewed complete-state handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Backend used during prompt ingestion.
    pub prefill: Backend,
    /// Backend used during token generation.
    pub decode: Backend,
}

impl Placement {
    const fn cpu_only(self) -> bool {
        matches!(self.prefill, Backend::Cpu) && matches!(self.decode, Backend::Cpu)
    }

    const fn accelerator_phases(self) -> u8 {
        let prefill = if matches!(self.prefill, Backend::Cpu) {
            0
        } else {
            1
        };
        let decode = if matches!(self.decode, Backend::Cpu) {
            0
        } else {
            1
        };
        prefill + decode
    }
}

/// Independently reviewed observations for one exact current compatibility key.
///
/// These values must come from trusted review, never directly from public API
/// requests. The recommendation remains a planning result, not a launch permit.
#[derive(Clone, Copy)]
pub struct Observation<'a> {
    /// Stable trusted profile identifier, used for deterministic ties.
    pub id: &'a str,
    /// Current device, driver, build, settings and workload identity.
    pub current: &'a ProfileCompatibilityKey,
    /// Archived receipt whose scope must exactly match the current identity.
    pub evidence: &'a ProfileEvidence,
    /// Independent review verified provenance and all affected promotion gates.
    pub independently_reviewed: bool,
    /// Current signed backend readiness and full resource admission both passed.
    pub ready_and_admitted: bool,
    /// Effective phase placement observed in the reviewed execution.
    pub placement: Placement,
    /// Independent migration/rollback qualification for a mixed-phase plan.
    pub phase_handoff_reviewed: bool,
    /// Digest of the common comparison settings, artifacts and exact workload.
    ///
    /// Unlike profile settings this excludes placement, but includes sampler,
    /// precision, adapter scale, actual token boundaries, cold/warm treatment,
    /// power conditions and the complete cost-accounting protocol.
    pub comparison_sha256: &'a str,
    /// Median complete-task nanoseconds, including setup and transition costs.
    pub complete_task_ns: u64,
    /// Conservative upper latency estimate retained by the reviewed comparison.
    pub upper_task_ns: u64,
    /// Measured median microjoules per acceptable task, including retries.
    pub energy_uj: Option<u64>,
    /// Conservative upper energy estimate; missing telemetry excludes battery ranking.
    pub upper_energy_uj: Option<u64>,
}

/// A provisional comparison result; contains no runtime arguments or permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recommendation<'a> {
    /// Best eligible measured profile under the requested objective.
    pub profile_id: &'a str,
    /// Effective prefill and decode placement, fixed for the complete request.
    pub placement: Placement,
}

fn same_task(left: &ProfileCompatibilityKey, right: &ProfileCompatibilityKey) -> bool {
    let left = &left.0;
    let right = &right.0;
    left.objective == right.objective
        && left.model_sha256 == right.model_sha256
        && left.adapter == right.adapter
        && left.capability_class_sha256 == right.capability_class_sha256
        && left.graph_contract == right.graph_contract
        && left.state_layout == right.state_layout
        && left.workload == right.workload
}

fn valid(observation: &Observation<'_>, control: &Observation<'_>) -> bool {
    observation.independently_reviewed
        && observation.ready_and_admitted
        && observation.evidence.0.verdict == Verdict::Passed
        && observation.evidence.applies_to(observation.current)
        && observation.current.0.runtime.target.is_windows_arm64()
        && same_task(observation.current, control.current)
        && super::identifier(observation.id)
        && super::super::hex(observation.comparison_sha256, 64)
        && observation.comparison_sha256 == control.comparison_sha256
        && observation.complete_task_ns > 0
        && observation.upper_task_ns >= observation.complete_task_ns
        && (observation.placement.prefill == observation.placement.decode
            || observation.phase_handoff_reviewed)
}

fn energy(observation: &Observation<'_>) -> Option<(u64, u64)> {
    if observation.evidence.0.energy_verdict != Verdict::Passed {
        return None;
    }
    let median = observation.energy_uj?;
    let upper = observation.upper_energy_uj?;
    (median > 0 && upper >= median).then_some((median, upper))
}

// Integer arithmetic makes the inclusive ten-percent gate reproducible and
// avoids both floating rounding and overflow at arbitrary measurement values.
fn improves_by_ten_percent(candidate_upper: u64, control_median: u64) -> bool {
    u128::from(candidate_upper) * 10 <= u128::from(control_median) * 9
}

/// Rank comparable observations against a reviewed CPU control.
///
/// Interactive selection requires at least ten percent lower complete-task
/// latency, using the candidate's conservative upper estimate. Battery selection
/// requires ten percent lower measured task energy and no latency regression.
/// A qualified GPU can beat an NPU on either X1 or X2; no SKU or TOPS shortcut
/// exists. Mixed prefill/decode placement competes as a complete measured plan,
/// including its transfers, rather than combining unrelated phase timings.
///
/// # Errors
/// Rejects an invalid/unreviewed/unavailable CPU control, duplicate profile IDs,
/// more than 32 candidates or missing CPU energy evidence for battery ranking.
/// Ineligible accelerator observations are excluded. No live serving caller is
/// wired to this offline recommendation until execution and promotion qualify.
pub fn recommend<'a>(
    control: &Observation<'a>,
    candidates: &[Observation<'a>],
) -> Result<Recommendation<'a>, String> {
    if !control.placement.cpu_only() || !valid(control, control) || candidates.len() > 32 {
        return Err("A reviewed and currently admitted CPU comparison is required.".into());
    }
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.id == control.id
            || candidates[..index]
                .iter()
                .any(|prior| prior.id == candidate.id)
        {
            return Err("Comparison profile identifiers must be unique.".into());
        }
    }
    let objective = control.current.0.objective;
    let control_score = match objective {
        Objective::Interactive => control.complete_task_ns,
        Objective::Battery => {
            energy(control)
                .ok_or("Battery ranking requires independently reviewed CPU energy.")?
                .0
        }
    };
    let mut best = control;
    let mut best_score = control_score;
    for candidate in candidates {
        if !valid(candidate, control) {
            continue;
        }
        let (score, upper) = match objective {
            Objective::Interactive => (candidate.complete_task_ns, candidate.upper_task_ns),
            Objective::Battery => {
                if candidate.upper_task_ns > control.complete_task_ns {
                    continue;
                }
                let Some(energy) = energy(candidate) else {
                    continue;
                };
                energy
            }
        };
        if !improves_by_ten_percent(upper, control_score) {
            continue;
        }
        // Ties retain CPU first, then prefer less accelerator placement, then
        // a stable identifier. Candidate iteration order never chooses a plan.
        let tie = (
            candidate.placement.accelerator_phases(),
            candidate.placement.prefill,
            candidate.placement.decode,
            candidate.id,
        );
        let best_tie = (
            best.placement.accelerator_phases(),
            best.placement.prefill,
            best.placement.decode,
            best.id,
        );
        if score < best_score || (score == best_score && tie < best_tie) {
            best = candidate;
            best_score = score;
        }
    }
    Ok(Recommendation {
        profile_id: best.id,
        placement: best.placement,
    })
}

#[cfg(test)]
mod tests;
