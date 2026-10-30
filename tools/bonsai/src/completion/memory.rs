//! Constant-space accounting from the supervisor's existing process queries.

use bonsai_windows_process::ProcessMemory;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Default)]
struct Aggregate {
    samples: u64,
    maximum_private: Option<usize>,
    maximum_working_set: Option<usize>,
    lifetime_peak_working_set: Option<usize>,
}

impl Aggregate {
    fn observe(&mut self, memory: ProcessMemory) {
        self.samples = self.samples.saturating_add(1);
        self.maximum_private = Some(
            self.maximum_private
                .unwrap_or_default()
                .max(memory.private_bytes),
        );
        self.maximum_working_set = Some(
            self.maximum_working_set
                .unwrap_or_default()
                .max(memory.working_set_bytes),
        );
        self.lifetime_peak_working_set = Some(
            self.lifetime_peak_working_set
                .unwrap_or_default()
                .max(memory.peak_working_set_bytes),
        );
    }

    fn sampled_receipt(&self) -> Value {
        json!({
            "Samples": self.samples,
            "SampledPeakPrivateBytes": self.maximum_private,
            "SampledPeakWorkingSetBytes": self.maximum_working_set,
        })
    }
}

#[derive(Default)]
struct OutputProgress {
    stdout_observations: u64,
    last_empty_milliseconds: Option<u128>,
    first_nonempty_milliseconds: Option<u128>,
    first_nonempty_after_exit: Option<bool>,
    before_output: Aggregate,
    after_output: Aggregate,
    after_exit: Aggregate,
}

/// Optional attribution uses observable file/exit boundaries, never token phases.
#[derive(Default)]
pub(super) struct Observation {
    total: Aggregate,
    output: Option<OutputProgress>,
}

impl Observation {
    pub(super) fn new(observe_output: bool) -> Self {
        Self {
            output: observe_output.then(OutputProgress::default),
            ..Self::default()
        }
    }

    pub(super) const fn observe_stdout(
        &mut self,
        bytes: u64,
        elapsed: Duration,
        root_exited: bool,
    ) {
        if let Some(output) = &mut self.output {
            output.stdout_observations = output.stdout_observations.saturating_add(1);
            if output.first_nonempty_milliseconds.is_none() {
                if bytes == 0 {
                    output.last_empty_milliseconds = Some(elapsed.as_millis());
                } else {
                    output.first_nonempty_milliseconds = Some(elapsed.as_millis());
                    output.first_nonempty_after_exit = Some(root_exited);
                }
            }
        }
    }

    pub(super) fn observe(&mut self, memory: ProcessMemory, root_exited: bool) {
        self.total.observe(memory);
        if let Some(output) = &mut self.output {
            let stage = if root_exited {
                &mut output.after_exit
            } else if output.first_nonempty_milliseconds.is_some() {
                &mut output.after_output
            } else {
                &mut output.before_output
            };
            stage.observe(memory);
        }
    }

    pub(super) fn receipt(&self, interval: Duration) -> Value {
        let mut receipt = self.total.sampled_receipt();
        receipt["NominalSamplingIntervalMilliseconds"] = json!(interval.as_millis());
        receipt["LifetimePeakWorkingSetBytes"] = json!(self.total.lifetime_peak_working_set);
        receipt["Scope"] = json!(
            "Root-process counters from existing safety queries and final-exit observation. Sampled maxima can miss brief peaks; OS lifetime peak working set includes mapped residency. Counters are not additive with Job commit or system available memory. No model-phase attribution or energy measurement."
        );
        if let Some(output) = &self.output {
            receipt["OutputProgress"] = json!({
                "AttributionOnly": true,
                "StdoutObservations": output.stdout_observations,
                "LastEmptyStdoutObservedMilliseconds": output.last_empty_milliseconds,
                "FirstNonemptyStdoutObservedMilliseconds": output.first_nonempty_milliseconds,
                "FirstNonemptyStdoutObservationAfterRootExit": output.first_nonempty_after_exit,
                "BeforeFirstNonemptyStdout": output.before_output.sampled_receipt(),
                "AfterFirstNonemptyStdout": output.after_output.sampled_receipt(),
                "AfterRootExit": output.after_exit.sampled_receipt(),
                "Scope": "Existing capped-stdout file-length checks and current-memory queries only. File output may include prompts, progress, buffering or final markers; this is not first-token latency or startup/prefill/decode attribution. An exit can race a running observation. Empty stages have unknown peaks; stage peaks are not additive. Lifetime working-set peaks are process-wide and never assigned to a stage. Instrumented attribution runs do not qualify timing comparisons.",
            });
        }
        receipt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_attribution_keeps_exit_and_lifetime_peaks_out_of_running_stages() {
        let mut observation = Observation::new(true);
        let memory = |private_bytes, working_set_bytes, peak_working_set_bytes| ProcessMemory {
            private_bytes,
            working_set_bytes,
            peak_working_set_bytes,
        };
        observation.observe_stdout(0, Duration::from_millis(1000), false);
        observation.observe(memory(100, 200, 900), false);
        observation.observe_stdout(3, Duration::from_millis(2000), false);
        observation.observe(memory(300, 150, 950), false);
        // A truncated file never moves the first-observation boundary backwards.
        observation.observe_stdout(0, Duration::from_millis(3000), false);
        observation.observe(memory(250, 100, 950), false);
        observation.observe_stdout(3, Duration::from_millis(3100), true);
        observation.observe(memory(0, 0, 1000), true);
        let receipt = observation.receipt(Duration::from_secs(1));
        let output = &receipt["OutputProgress"];
        assert_eq!(receipt["Samples"], 4);
        assert_eq!(receipt["LifetimePeakWorkingSetBytes"], 1000);
        assert_eq!(output["StdoutObservations"], 4);
        assert_eq!(output["LastEmptyStdoutObservedMilliseconds"], 1000);
        assert_eq!(output["FirstNonemptyStdoutObservedMilliseconds"], 2000);
        assert_eq!(output["FirstNonemptyStdoutObservationAfterRootExit"], false);
        assert_eq!(output["BeforeFirstNonemptyStdout"]["Samples"], 1);
        assert_eq!(output["AfterFirstNonemptyStdout"]["Samples"], 2);
        assert_eq!(output["AfterRootExit"]["Samples"], 1);
        assert_eq!(
            output["BeforeFirstNonemptyStdout"]["SampledPeakPrivateBytes"],
            100
        );
        assert_eq!(
            output["AfterFirstNonemptyStdout"]["SampledPeakPrivateBytes"],
            300
        );
        assert_eq!(output["AfterRootExit"]["SampledPeakPrivateBytes"], 0);
        for stage in [
            "BeforeFirstNonemptyStdout",
            "AfterFirstNonemptyStdout",
            "AfterRootExit",
        ] {
            assert!(output[stage].get("LifetimePeakWorkingSetBytes").is_none());
        }
    }

    #[test]
    fn output_seen_only_after_exit_leaves_running_stages_unknown() {
        let mut observation = Observation::new(true);
        observation.observe_stdout(5, Duration::from_millis(500), true);
        observation.observe(
            ProcessMemory {
                private_bytes: 0,
                working_set_bytes: 0,
                peak_working_set_bytes: 100,
            },
            true,
        );
        let receipt = observation.receipt(Duration::from_secs(1));
        let output = &receipt["OutputProgress"];
        assert_eq!(output["FirstNonemptyStdoutObservationAfterRootExit"], true);
        assert!(output["LastEmptyStdoutObservedMilliseconds"].is_null());
        for stage in ["BeforeFirstNonemptyStdout", "AfterFirstNonemptyStdout"] {
            assert_eq!(output[stage]["Samples"], 0);
            assert!(output[stage]["SampledPeakPrivateBytes"].is_null());
            assert!(output[stage]["SampledPeakWorkingSetBytes"].is_null());
        }
        let empty = Observation::new(true).receipt(Duration::from_secs(1));
        assert!(empty["OutputProgress"]["FirstNonemptyStdoutObservedMilliseconds"].is_null());
        assert_eq!(empty["OutputProgress"]["StdoutObservations"], 0);
    }

    #[test]
    fn memory_observations_keep_sampled_and_lifetime_counter_domains_separate() {
        let mut observation = Observation::default();
        let empty = observation.receipt(Duration::from_secs(1));
        assert_eq!(empty["Samples"], 0);
        assert!(empty["SampledPeakPrivateBytes"].is_null());
        assert!(empty["SampledPeakWorkingSetBytes"].is_null());
        assert!(empty["LifetimePeakWorkingSetBytes"].is_null());

        for memory in [
            ProcessMemory {
                private_bytes: 100,
                working_set_bytes: 250,
                peak_working_set_bytes: 900,
            },
            ProcessMemory {
                private_bytes: 200,
                working_set_bytes: 150,
                peak_working_set_bytes: 950,
            },
            // A final post-exit query may report zero current usage. It must
            // neither erase earlier observations nor supply an observed peak.
            ProcessMemory {
                private_bytes: 0,
                working_set_bytes: 0,
                peak_working_set_bytes: 0,
            },
        ] {
            observation.observe(memory, false);
        }
        let receipt = observation.receipt(Duration::from_secs(1));
        assert_eq!(receipt["Samples"], 3);
        assert_eq!(receipt["NominalSamplingIntervalMilliseconds"], 1000);
        assert_eq!(receipt["SampledPeakPrivateBytes"], 200);
        assert_eq!(receipt["SampledPeakWorkingSetBytes"], 250);
        assert_eq!(receipt["LifetimePeakWorkingSetBytes"], 950);
        assert!(receipt.get("OutputProgress").is_none());
        assert_eq!(
            Observation::default().receipt(Duration::from_secs(1)),
            empty
        );
    }
}
