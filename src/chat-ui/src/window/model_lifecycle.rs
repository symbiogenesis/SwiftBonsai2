//! Admission and teardown state for the single owned model worker.

use crate::client::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelLifecycle {
    Unavailable,
    Preparing,
    Restarting,
    Ready,
    Generating,
    AwaitingTeardown,
    Stopping,
    Retryable,
}

impl ModelLifecycle {
    pub(super) const fn ready(self) -> bool {
        matches!(self, Self::Ready | Self::Generating)
    }

    pub(super) const fn busy(self) -> bool {
        matches!(self, Self::Generating)
    }

    pub(super) const fn stopping(self) -> bool {
        matches!(self, Self::Stopping)
    }

    pub(super) const fn retryable(self) -> bool {
        matches!(self, Self::Retryable)
    }

    pub(super) const fn waiting_model(self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Restarting | Self::AwaitingTeardown | Self::Stopping
        )
    }

    /// Events arrive in order from one worker. Retry is sent only after its
    /// teardown notice; attaching a replacement worker replaces its channel.
    /// Reject stale events that cannot belong to the current lifecycle phase.
    pub(super) const fn accepts(self, event: &Event) -> bool {
        match event {
            Event::Starting => matches!(self, Self::Preparing | Self::Restarting),
            Event::Ready => matches!(self, Self::Preparing),
            Event::Text(_)
            | Event::Reasoning(_)
            | Event::Stage(_)
            | Event::Draft(_)
            | Event::RetainedAnswer(_)
            | Event::Complete(_) => matches!(self, Self::Generating),
            Event::Committed(_) | Event::Stopping(_) => {
                matches!(self, Self::Generating | Self::Stopping)
            }
            Event::Stopped => matches!(
                self,
                Self::Stopping | Self::AwaitingTeardown | Self::Retryable
            ),
            Event::Rejected { retry_required, .. } => {
                matches!(self, Self::Generating | Self::Stopping)
                    || (*retry_required
                        && matches!(
                            self,
                            Self::AwaitingTeardown | Self::Retryable | Self::Restarting
                        ))
            }
            Event::Retryable(_) => !matches!(self, Self::Unavailable | Self::Restarting),
            Event::Failed(_) => !matches!(self, Self::Unavailable),
        }
    }
}
