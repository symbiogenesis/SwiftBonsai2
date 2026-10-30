//! Fixed-size, service-wide authenticated-request admission budget.

use std::{sync::Mutex, time::Duration};
use tokio::time::Instant;

const BURST: Duration = Duration::from_secs(8);
const REQUEST: Duration = Duration::from_secs(1);

/// One shared token bucket, independent of client identity or request contents.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the crate-only re-export must not expose this private service implementation publicly"
)]
pub(crate) struct BurstBudget {
    state: Mutex<State>,
}

struct State {
    credit: Duration,
    updated: Instant,
}

impl BurstBudget {
    pub(crate) const fn new(now: Instant) -> Self {
        Self {
            state: Mutex::new(State {
                credit: BURST,
                updated: now,
            }),
        }
    }

    /// Spend one request, preserving fractional refill credit across denials.
    /// Poisoned or backward-time observations fail closed without allocating.
    pub(crate) fn take(&self, now: Instant) -> Result<(), Duration> {
        let Ok(mut state) = self.state.lock() else {
            return Err(REQUEST);
        };
        let Some(elapsed) = now.checked_duration_since(state.updated) else {
            return Err(REQUEST);
        };
        state.credit = state.credit.saturating_add(elapsed).min(BURST);
        state.updated = now;
        if let Some(remaining) = state.credit.checked_sub(REQUEST) {
            state.credit = remaining;
            Ok(())
        } else {
            Err(REQUEST.saturating_sub(state.credit))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BURST, BurstBudget, REQUEST};
    use std::{sync::Arc, time::Duration};
    use tokio::time::Instant;

    #[test]
    fn burst_budget_preserves_fractional_credit_during_sustained_denials() {
        let now = Instant::now();
        let budget = BurstBudget::new(now);
        for _ in 0..8 {
            assert_eq!(budget.take(now), Ok(()));
        }
        assert_eq!(budget.take(now), Err(REQUEST));
        for millisecond in 1..1000 {
            assert_eq!(
                budget.take(now + Duration::from_millis(millisecond)),
                Err(Duration::from_millis(1000 - millisecond))
            );
        }
        assert_eq!(budget.take(now + REQUEST), Ok(()));
        assert_eq!(budget.take(now + REQUEST), Err(REQUEST));
        // Idle time saturates rather than accumulating an unlimited burst.
        let later = now + BURST * 100;
        for _ in 0..8 {
            assert_eq!(budget.take(later), Ok(()));
        }
        assert_eq!(budget.take(later), Err(REQUEST));
        assert_eq!(budget.take(now), Err(REQUEST));
        assert_eq!(budget.take(later), Err(REQUEST));
    }

    #[test]
    fn burst_budget_shares_one_hard_budget_across_concurrent_callers() {
        let now = Instant::now();
        let budget = Arc::new(BurstBudget::new(now));
        let outcomes = std::thread::scope(|scope| {
            #[expect(
                clippy::needless_collect,
                reason = "start every contender before the first join blocks"
            )]
            let calls: Vec<_> = (0..32)
                .map(|_| scope.spawn(|| budget.take(now).is_ok()))
                .collect();
            calls
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect::<Result<Vec<_>, _>>()
        });
        assert!(
            matches!(outcomes, Ok(calls) if calls.iter().filter(|allowed| **allowed).count() == 8)
        );
        assert_eq!(budget.take(now), Err(REQUEST));
    }

    #[test]
    fn burst_budget_rejected_body_drain_retains_absolute_deadline() -> std::io::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()?;
        runtime.block_on(async {
            let (_client, mut server) = tokio::io::duplex(16);
            let deadline = Instant::now() + REQUEST;
            assert_eq!(
                crate::reception::discard_rate_body(&mut server, 1024 * 1024, deadline).await,
                Err(crate::reception::ReceiveError::Deadline)
            );
            assert_eq!(Instant::now(), deadline);
        });
        Ok(())
    }
}
