#![forbid(unsafe_code)]
//! Bounded, replayable scheduler histories against a semaphore-free model.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and model invariants must fail immediately"
)]

use bonsai_broker::scheduler::{ActiveRequest, GateError, MAX_WAITING, RequestGate, Reservation};
use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use tokio::time::{Instant, advance};

const SLOTS: usize = MAX_WAITING + 3;
type Entry<'a> = Pin<Box<dyn Future<Output = Result<ActiveRequest<'a>, GateError>> + Send + 'a>>;

enum Actual<'a> {
    Empty,
    Reserved(Reservation<'a>),
    Entering(Entry<'a>),
    Active(ActiveRequest<'a>),
}

#[derive(Clone, Copy, Debug)]
enum Expected {
    Empty,
    Reserved,
    Entering { deadline: u64, polled: bool },
    Active { deadline: u64 },
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Reserve(usize),
    Enter(usize, u64),
    Poll(usize),
    Drop(usize),
    Advance(u64),
    Close,
}

/// The owner can be an active guard or a waiter granted the slot but not polled.
/// Expiry alone does not release ownership: the owner must be polled or dropped.
struct Model {
    slots: [Expected; SLOTS],
    queue: VecDeque<usize>,
    owner: Option<usize>,
    now: u64,
    closed: bool,
}

impl Model {
    const fn new() -> Self {
        Self {
            slots: [Expected::Empty; SLOTS],
            queue: VecDeque::new(),
            owner: None,
            now: 0,
            closed: false,
        }
    }

    fn occupied(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| !matches!(slot, Expected::Empty))
            .count()
    }

    fn release(&mut self, id: usize) {
        self.slots[id] = Expected::Empty;
        self.queue.retain(|queued| *queued != id);
        if self.owner == Some(id) {
            self.owner = self.queue.pop_front();
        }
    }

    fn poll(&mut self, id: usize) -> Poll<Result<(), GateError>> {
        let Expected::Entering { deadline, polled } = self.slots[id] else {
            unreachable!("only entering slots are polled")
        };
        let error = if self.closed {
            Some(GateError::Closed)
        } else if self.now >= deadline {
            Some(GateError::Deadline)
        } else {
            None
        };
        if let Some(error) = error {
            self.release(id);
            return Poll::Ready(Err(error));
        }
        if !polled {
            self.slots[id] = Expected::Entering {
                deadline,
                polled: true,
            };
            if self.owner.is_none() {
                self.owner = Some(id);
            } else {
                self.queue.push_back(id);
            }
        }
        if self.owner == Some(id) {
            self.slots[id] = Expected::Active { deadline };
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

struct History<'a> {
    gate: &'a RequestGate,
    actual: [Actual<'a>; SLOTS],
    model: Model,
    origin: Instant,
    capacity: usize,
    seed: u64,
    trace: Vec<Operation>,
}

impl std::fmt::Debug for History<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("History")
            .field("seed", &self.seed)
            .field("capacity", &self.capacity)
            .field("trace", &self.trace)
            .finish_non_exhaustive()
    }
}

impl<'a> History<'a> {
    fn new(gate: &'a RequestGate, waiting: usize, seed: u64) -> Self {
        Self {
            gate,
            actual: std::array::from_fn(|_| Actual::Empty),
            model: Model::new(),
            origin: Instant::now(),
            capacity: waiting + 1,
            seed,
            trace: Vec::with_capacity(192),
        }
    }

    fn instant(&self, tick: u64) -> Instant {
        self.origin + Duration::from_millis(tick)
    }

    fn poll(&mut self, id: usize) {
        let expected = self.model.poll(id);
        let Actual::Entering(future) = &mut self.actual[id] else {
            unreachable!("actual and model slot kinds agree")
        };
        let observed = match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(active)) => {
                self.actual[id] = Actual::Active(active);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                self.actual[id] = Actual::Empty;
                Poll::Ready(Err(error))
            }
        };
        assert_eq!(observed, expected, "{self:?}");
    }

    /// Probe every unoccupied reservation, not just the full/empty extremes.
    /// Temporary reservations never enter the active queue.
    fn check(&self) {
        assert_eq!(self.gate.is_closed(), self.model.closed, "{self:?}");
        let mut probes = Vec::with_capacity(self.capacity);
        if !self.model.closed {
            for _ in self.model.occupied()..self.capacity {
                let Ok(reservation) = self.gate.reserve() else {
                    unreachable!("reservation capacity leaked: {self:?}")
                };
                probes.push(reservation);
            }
            assert_eq!(probes.len(), self.capacity - self.model.occupied());
        }
        let error = self.gate.reserve().err();
        assert_eq!(
            error,
            Some(if self.model.closed {
                GateError::Closed
            } else {
                GateError::Capacity
            }),
            "{self:?}"
        );
        drop(probes);
        let mut active_count = 0;
        for (actual, expected) in self.actual.iter().zip(self.model.slots) {
            if let Actual::Active(active) = actual {
                active_count += 1;
                let Expected::Active { deadline } = expected else {
                    unreachable!("active guards agree with model")
                };
                assert_eq!(active.deadline(), self.instant(deadline), "{self:?}");
                let expected = if self.model.closed {
                    Err(GateError::Closed)
                } else if self.model.now >= deadline {
                    Err(GateError::Deadline)
                } else {
                    Ok(())
                };
                assert_eq!(active.check(), expected, "{self:?}");
            }
        }
        assert!(active_count <= 1, "{self:?}");
    }

    async fn apply(&mut self, operation: Operation) {
        self.trace.push(operation);
        match operation {
            Operation::Reserve(id) => {
                assert!(matches!(self.model.slots[id], Expected::Empty), "{self:?}");
                let expected = if self.model.closed {
                    Err(GateError::Closed)
                } else if self.model.occupied() == self.capacity {
                    Err(GateError::Capacity)
                } else {
                    Ok(())
                };
                let actual = self.gate.reserve();
                assert_eq!(
                    actual.as_ref().map(|_| ()).map_err(|error| *error),
                    expected,
                    "{self:?}"
                );
                if let Ok(reservation) = actual {
                    self.actual[id] = Actual::Reserved(reservation);
                    self.model.slots[id] = Expected::Reserved;
                }
            }
            Operation::Enter(id, budget) => {
                let Actual::Reserved(reservation) =
                    std::mem::replace(&mut self.actual[id], Actual::Empty)
                else {
                    unreachable!("enter consumes a reservation")
                };
                let deadline = self.model.now + budget;
                self.actual[id] =
                    Actual::Entering(Box::pin(reservation.enter(self.instant(deadline))));
                self.model.slots[id] = Expected::Entering {
                    deadline,
                    polled: false,
                };
            }
            Operation::Poll(id) => self.poll(id),
            Operation::Drop(id) => {
                self.actual[id] = Actual::Empty;
                self.model.release(id);
            }
            Operation::Advance(ticks) => {
                advance(Duration::from_millis(ticks)).await;
                self.model.now += ticks;
            }
            Operation::Close => {
                self.gate.close();
                self.model.closed = true;
            }
        }
        self.check();
    }

    fn next(&self, random: &mut u64) -> Operation {
        // Fixed xorshift is replayable across Rust versions and platforms.
        *random ^= *random << 13;
        *random ^= *random >> 7;
        *random ^= *random << 17;
        let id = usize::try_from(*random % u64::try_from(SLOTS).unwrap()).unwrap();
        let choice = (*random >> 32) % 8;
        if choice == 0 {
            return Operation::Advance(1 + (*random % 3));
        }
        match self.model.slots[id] {
            Expected::Empty => Operation::Reserve(id),
            Expected::Reserved if choice == 1 => Operation::Drop(id),
            Expected::Reserved => Operation::Enter(id, (*random >> 16) % 9),
            Expected::Entering { .. } if choice <= 2 => Operation::Drop(id),
            Expected::Entering { .. } => Operation::Poll(id),
            Expected::Active { .. } => Operation::Drop(id),
        }
    }

    async fn shutdown(&mut self) {
        self.apply(Operation::Close).await;
        self.apply(Operation::Close).await;
        for id in 0..SLOTS {
            match self.model.slots[id] {
                Expected::Reserved => {
                    self.apply(Operation::Enter(id, 0)).await;
                    self.apply(Operation::Poll(id)).await;
                }
                Expected::Entering { .. } => self.apply(Operation::Poll(id)).await,
                Expected::Empty | Expected::Active { .. } => {}
            }
            self.apply(Operation::Drop(id)).await;
        }
        self.apply(Operation::Advance(10)).await;
        self.apply(Operation::Reserve(0)).await;
        self.gate.closed().await;
    }
}

#[test]
fn seeded_admission_histories_match_reference_model() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            // 144 bounded histories, 18,432 generated operations, no tasks,
            // sockets, wall-clock sleeps, random dependency or model processes.
            for waiting in 0..=MAX_WAITING {
                for seed in 1..=16 {
                    let gate = RequestGate::new(waiting).unwrap();
                    let mut history = History::new(&gate, waiting, seed);
                    let mut random = seed;
                    for _ in 0..128 {
                        let operation = history.next(&mut random);
                        history.apply(operation).await;
                    }
                    history.shutdown().await;
                    drop(history);
                }
            }
        });
}

#[test]
fn reordered_polling_cancellation_and_deadline_edges_match_reference_model() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            use Operation::{Advance, Drop, Enter, Poll, Reserve};

            let gate = RequestGate::new(3).unwrap();
            let mut history = History::new(&gate, 3, 0);
            for operation in [
                Reserve(0),
                Reserve(1),
                Reserve(2),
                Reserve(3),
                Reserve(4),
                Enter(0, 50),
                Poll(0),
                Enter(1, 50),
                Enter(2, 50),
                // Reservation 2 joins first; reservation 1 is still unpolled.
                Poll(2),
                Poll(1),
                Enter(3, 50),
                Drop(3),
                // A later poll cannot steal the slot assigned to waiter 2.
                Drop(0),
                Poll(1),
                Drop(2),
                Poll(1),
                Drop(1),
                Reserve(0),
                Enter(0, 3),
                Poll(0),
                Reserve(1),
                Enter(1, 2),
                Poll(1),
                Advance(1),
                Poll(1),
                Advance(1),
                Poll(1),
                // Active expiry does not release its reservation or active slot.
                Reserve(1),
                Enter(1, 20),
                Poll(1),
                Advance(1),
                Poll(1),
                Drop(0),
                Poll(1),
                Drop(1),
                // Expired unpolled entries reject even an idle active slot.
                Reserve(0),
                Enter(0, 1),
                Advance(1),
                Poll(0),
                // A granted queue entry still checks its original deadline.
                Reserve(0),
                Enter(0, 20),
                Poll(0),
                Reserve(1),
                Enter(1, 1),
                Poll(1),
                Drop(0),
                Advance(1),
                Poll(1),
                Reserve(0),
                Enter(0, 20),
                Poll(0),
                Reserve(1),
                Enter(1, 1),
                Poll(1),
                Advance(1),
                Reserve(2),
                Enter(2, 0),
            ] {
                history.apply(operation).await;
            }
            // Close takes precedence over simultaneous expiry, including an
            // existing active guard, queued entry and unpolled entry.
            history.shutdown().await;
            drop(history);
        });
}
