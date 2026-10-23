//! Bounded admission and one active request, without retaining request payloads.
//!
//! Reserve before accepting or reading a client stream or allocating its body.
//! The reservation counts against the total accepted-connection budget until
//! dropped, including while receiving, waiting and serving. First poll of
//! [`Reservation::enter`] joins Tokio's fair single-permit queue; reservation
//! order alone does not order requests whose reception completes at different
//! times. No task, listener, request queue or model process is created here.

use tokio::{
    sync::{Notify, Semaphore, SemaphorePermit, TryAcquireError},
    time::{Instant, timeout_at},
};

/// Largest permitted number of additional accepted requests beside the active one.
pub const MAX_WAITING: usize = 8;

/// Value-free scheduling errors that contain no client or backend information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateError {
    /// The configured waiting budget exceeds the fixed supported bound.
    Configuration,
    /// All accepted-request reservations are occupied; reject without waiting.
    Capacity,
    /// The gate is shut down; reservations and queued entries are rejected.
    Closed,
    /// The supplied absolute request deadline has expired.
    Deadline,
}

/// One active request plus a fixed, bounded number of additional reservations.
///
/// This is an admission primitive, not a cancellation supervisor. The owner must
/// keep the reservation/active guard until its client and backend resources have
/// been closed. Closing the gate wakes queued entries and prevents new ones;
/// active work must observe [`ActiveRequest::check`] or be cancelled by its owner.
pub struct RequestGate {
    capacity: Semaphore,
    active: Semaphore,
    shutdown: Notify,
}

impl RequestGate {
    /// Construct a single-active gate with zero to [`MAX_WAITING`] extra slots.
    ///
    /// A zero waiting budget still allows one accepted request. Reservation
    /// counts include requests that have not started reception or joined the
    /// active queue; an idle active slot never permits exceeding this total.
    ///
    /// # Errors
    /// Returns [`GateError::Configuration`] for an excessive or overflowing bound.
    pub fn new(max_waiting: usize) -> Result<Self, GateError> {
        let capacity = max_waiting
            .checked_add(1)
            .filter(|_| max_waiting <= MAX_WAITING)
            .ok_or(GateError::Configuration)?;
        Ok(Self {
            capacity: Semaphore::new(capacity),
            active: Semaphore::new(1),
            shutdown: Notify::new(),
        })
    }

    /// Reserve an accepted-request slot immediately, before any request I/O.
    ///
    /// The returned guard releases capacity on drop, including cancellation
    /// before calling or polling [`Reservation::enter`]. This call never waits.
    ///
    /// # Errors
    /// Returns [`GateError::Capacity`] when full or [`GateError::Closed`] after
    /// shutdown begins. No request payload or pending acquisition is retained.
    pub fn reserve(&self) -> Result<Reservation<'_>, GateError> {
        let capacity = self.capacity.try_acquire().map_err(|error| match error {
            TryAcquireError::NoPermits => GateError::Capacity,
            TryAcquireError::Closed => GateError::Closed,
        })?;
        // Close both semaphores fail-closed even if reserve races with close.
        if self.is_closed() {
            return Err(GateError::Closed);
        }
        Ok(Reservation {
            gate: self,
            capacity,
        })
    }

    /// Permanently reject new reservations and wake queued entries as closed.
    ///
    /// Idempotent. Existing guards retain their slots until dropped; this method
    /// does not claim that their owned client/backend resources were cancelled.
    pub fn close(&self) {
        self.capacity.close();
        self.active.close();
        self.shutdown.notify_waiters();
    }

    /// Whether permanent shutdown has begun.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.capacity.is_closed() || self.active.is_closed()
    }

    /// Wait until permanent shutdown begins, including when already closed.
    ///
    /// Multiple concurrent callers are supported. The supervisor can race this
    /// future against reception or active work to drop owned resources on close.
    /// Cancelling the wait has no side effects on the gate or other waiters.
    pub async fn closed(&self) {
        // notify_waiters covers every Notified created before the notification,
        // even if not polled yet. Register before checking the durable state.
        let notified = self.shutdown.notified();
        if !self.is_closed() {
            notified.await;
        }
    }
}

/// An accepted-request slot released automatically on cancellation or drop.
#[must_use = "dropping the reservation releases its accepted-request slot"]
pub struct Reservation<'a> {
    gate: &'a RequestGate,
    capacity: SemaphorePermit<'a>,
}

impl<'a> Reservation<'a> {
    /// Wait fairly for the sole active slot under one absolute request deadline.
    ///
    /// Requires a Tokio time driver. Queue order follows the first polling of
    /// this future, not when the reservation was made. Dropping the future
    /// removes its queue entry and releases its accepted-request reservation.
    /// The same deadline must cover reception, counting, generation and response
    /// writing; calling this method must not create a fresh request-time budget.
    ///
    /// # Errors
    /// Returns [`GateError::Closed`] when shut down, or [`GateError::Deadline`]
    /// if the deadline expires before or immediately after slot acquisition.
    pub async fn enter(self, deadline: Instant) -> Result<ActiveRequest<'a>, GateError> {
        self.gate.check(deadline)?;
        let active = timeout_at(deadline, self.gate.active.acquire())
            .await
            .map_err(|_| GateError::Deadline)?
            .map_err(|_| GateError::Closed)?;
        // An immediately ready acquisition can beat timeout_at after expiry.
        self.gate.check(deadline)?;
        Ok(ActiveRequest {
            gate: self.gate,
            deadline,
            _active: active,
            _capacity: self.capacity,
        })
    }
}

impl RequestGate {
    fn check(&self, deadline: Instant) -> Result<(), GateError> {
        if self.is_closed() {
            return Err(GateError::Closed);
        }
        if Instant::now() >= deadline {
            return Err(GateError::Deadline);
        }
        Ok(())
    }
}

/// The sole active slot and its accepted-request reservation, released on drop.
///
/// Hold this guard through token counting, generation, response delivery and
/// closing owned streams. If cancellation leaves backend work alive, the owner
/// must establish backend cancellation before allowing another generation.
#[must_use = "dropping the guard releases the active and accepted-request slots"]
pub struct ActiveRequest<'a> {
    gate: &'a RequestGate,
    deadline: Instant,
    // Release the active slot before capacity when normal field drop runs.
    _active: SemaphorePermit<'a>,
    _capacity: SemaphorePermit<'a>,
}

impl ActiveRequest<'_> {
    /// The unchanged absolute deadline supplied when joining the active queue.
    #[must_use]
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Check for shutdown and expiry before and after work that may be ready.
    ///
    /// This does not release either permit: the owner must first close or
    /// cancel its resources and then drop the guard.
    ///
    /// # Errors
    /// Returns [`GateError::Closed`] after shutdown starts, otherwise
    /// [`GateError::Deadline`] at or beyond the supplied absolute deadline.
    pub fn check(&self) -> Result<(), GateError> {
        self.gate.check(self.deadline)
    }
}
