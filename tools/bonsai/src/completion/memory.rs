//! Constant-space accounting from the supervisor's existing process queries.

use bonsai_windows_process::ProcessMemory;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Default)]
pub(super) struct Observation {
    samples: u64,
    maximum_private: Option<usize>,
    maximum_working_set: Option<usize>,
    lifetime_peak_working_set: Option<usize>,
}

impl Observation {
    pub(super) fn observe(&mut self, memory: ProcessMemory) {
        self.samples += 1;
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

    pub(super) fn receipt(&self, interval: Duration) -> Value {
        json!({
            "Samples": self.samples,
            "NominalSamplingIntervalMilliseconds": interval.as_millis(),
            "SampledPeakPrivateBytes": self.maximum_private,
            "SampledPeakWorkingSetBytes": self.maximum_working_set,
            "LifetimePeakWorkingSetBytes": self.lifetime_peak_working_set,
            "Scope": "Root-process counters from existing safety queries and final-exit observation. Sampled maxima can miss brief peaks; OS lifetime peak working set includes mapped residency. Counters are not additive with Job commit or system available memory. No phase attribution or energy measurement.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            observation.observe(memory);
        }
        let receipt = observation.receipt(Duration::from_secs(1));
        assert_eq!(receipt["Samples"], 3);
        assert_eq!(receipt["NominalSamplingIntervalMilliseconds"], 1000);
        assert_eq!(receipt["SampledPeakPrivateBytes"], 200);
        assert_eq!(receipt["SampledPeakWorkingSetBytes"], 250);
        assert_eq!(receipt["LifetimePeakWorkingSetBytes"], 950);
        assert_eq!(
            Observation::default().receipt(Duration::from_secs(1)),
            empty
        );
    }
}
