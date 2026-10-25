#![forbid(unsafe_code)]
//! Regression coverage for bounded single-active request admission.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::scheduler::{GateError, MAX_WAITING, RequestGate};
use std::{future::poll_fn, pin::pin, task::Poll, time::Duration};
use tokio::time::{Instant, advance};

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[test]
fn waiting_limit_rejects_invalid_configuration() {
    // The model histories probe reservation capacity and its recovery for
    // every supported waiting limit after each operation.
    for invalid in [MAX_WAITING + 1, usize::MAX] {
        assert!(matches!(
            RequestGate::new(invalid),
            Err(GateError::Configuration)
        ));
    }
}

#[test]
fn entries_are_fifo_and_only_one_request_is_active() {
    run(async {
        let gate = RequestGate::new(2).unwrap();
        let active = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        let mut first = pin!(gate.reserve().unwrap().enter(deadline()));
        let mut second = pin!(gate.reserve().unwrap().enter(deadline()));
        poll_fn(|context| {
            assert!(first.as_mut().poll(context).is_pending());
            assert!(second.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(active);
        // Polling the later waiter first must not let it overtake the first.
        poll_fn(|context| {
            assert!(second.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        let first = first.await.unwrap();
        let extra = gate.reserve().unwrap();
        assert!(matches!(gate.reserve(), Err(GateError::Capacity)));
        poll_fn(|context| {
            assert!(second.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(first);
        let second = second.await.unwrap();
        assert!(second.check().is_ok());
        drop(extra);
        assert!(gate.reserve().is_ok());
        drop(second);
    });
}

#[test]
fn expired_deadlines_reject_ready_entry_and_release_capacity() {
    run(async {
        let gate = RequestGate::new(0).unwrap();
        let expired = Instant::now();
        assert!(matches!(
            gate.reserve().unwrap().enter(expired).await,
            Err(GateError::Deadline)
        ));
        let active = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        let limit = active.deadline();
        advance(Duration::from_secs(60)).await;
        assert_eq!(active.deadline(), limit);
        assert_eq!(active.check(), Err(GateError::Deadline));
        assert!(matches!(gate.reserve(), Err(GateError::Capacity)));
        drop(active);
        assert!(gate.reserve().is_ok());
    });
}

#[test]
fn queued_deadline_does_not_reset_and_releases_its_reservation() {
    run(async {
        let gate = RequestGate::new(1).unwrap();
        let active = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        let expires = Instant::now() + Duration::from_secs(5);
        let mut queued = pin!(gate.reserve().unwrap().enter(expires));
        poll_fn(|context| {
            assert!(queued.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        advance(Duration::from_secs(5)).await;
        assert!(matches!(queued.await, Err(GateError::Deadline)));
        assert!(gate.reserve().is_ok());
        assert!(active.check().is_ok());
        drop(active);
    });
}

#[test]
fn cancellation_releases_unpolled_queued_and_active_slots() {
    run(async {
        let gate = RequestGate::new(2).unwrap();
        let active = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        let unpolled = gate.reserve().unwrap().enter(deadline());
        let mut cancelled = Box::pin(gate.reserve().unwrap().enter(deadline()));
        assert!(matches!(gate.reserve(), Err(GateError::Capacity)));
        poll_fn(|context| {
            assert!(cancelled.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(unpolled);
        let mut surviving = pin!(gate.reserve().unwrap().enter(deadline()));
        poll_fn(|context| {
            assert!(surviving.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(cancelled);
        assert!(gate.reserve().is_ok());
        drop(active);
        let surviving = surviving.await.unwrap();
        assert!(surviving.check().is_ok());
        drop(surviving);
        // Both semaphore permits must return after the active guard is dropped.
        let next = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        assert!(next.check().is_ok());
        drop(next);
    });
}

#[test]
fn shutdown_rejects_waiters_reservations_and_active_checks() {
    run(async {
        let gate = RequestGate::new(2).unwrap();
        let active = gate.reserve().unwrap().enter(deadline()).await.unwrap();
        let reserved = gate.reserve().unwrap();
        let mut queued = pin!(gate.reserve().unwrap().enter(deadline()));
        let mut first_closed = pin!(gate.closed());
        let mut second_closed = pin!(gate.closed());
        poll_fn(|context| {
            assert!(queued.as_mut().poll(context).is_pending());
            assert!(first_closed.as_mut().poll(context).is_pending());
            assert!(second_closed.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        gate.close();
        gate.close();
        first_closed.await;
        second_closed.await;
        gate.closed().await;
        assert!(gate.is_closed());
        assert!(matches!(gate.reserve(), Err(GateError::Closed)));
        assert!(matches!(queued.await, Err(GateError::Closed)));
        assert!(matches!(
            reserved.enter(deadline()).await,
            Err(GateError::Closed)
        ));
        assert_eq!(active.check(), Err(GateError::Closed));
        drop(active);
        assert!(matches!(gate.reserve(), Err(GateError::Closed)));
    });
}
