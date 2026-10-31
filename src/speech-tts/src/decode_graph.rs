//! Rust ownership of the pinned Qwen CPU decode workspace.
//!
//! The owner retains one completed native graph, releases it before replacement,
//! and prevents eviction or teardown while a synchronous compute lease is active.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep graph ownership private to the speech crate and audited ABI."
)]

/// Select the zero-based 256-token window without signed addition overflow.
pub(super) fn window_index(padded_tokens: i32, windows: usize) -> Option<usize> {
    let tokens = usize::try_from(padded_tokens).ok().filter(|&n| n != 0)?;
    let index = (tokens - 1) / 256;
    (index < windows).then_some(index)
}

/// One Rust-owned resource and its valid geometry, independent of persistent KV.
pub(super) struct Cache<T> {
    windows: usize,
    current: Option<((usize, i32), T)>,
    active: bool,
}

impl<T> Cache<T> {
    /// Create an empty cache without allocating one slot per window.
    pub(super) const fn new(windows: usize) -> Option<Self> {
        if windows == 0 {
            return None;
        }
        Some(Self {
            windows,
            current: None,
            active: false,
        })
    }

    /// Borrow one resource until completion, rebuilding only after eviction.
    ///
    /// The builder executes after the previous resource's destructor. Failure
    /// leaves an empty cache, so retry cannot reuse partial or stale workspace.
    pub(super) fn begin(
        &mut self,
        padded_tokens: i32,
        width: i32,
        build: impl FnOnce() -> Option<T>,
    ) -> Option<&mut T> {
        if self.active || width <= 0 {
            return None;
        }
        let key = (window_index(padded_tokens, self.windows)?, width);
        if let Some((cached, resource)) = self.current.as_mut()
            && *cached == key
        {
            self.active = true;
            return Some(resource);
        }
        self.current = None;
        let resource = build()?;
        self.active = true;
        Some(&mut self.current.insert((key, resource)).1)
    }

    /// End a lease only after native graph work and output reads synchronize.
    pub(super) const fn complete(&mut self) -> bool {
        let was_active = self.active;
        self.active = false;
        was_active
    }
}

impl<T> Drop for Cache<T> {
    fn drop(&mut self) {
        if self.active {
            // An outstanding native borrow may still be computing. Never free
            // its memory or recover by replaying against another workspace.
            std::process::abort();
        }
    }
}

#[cfg(test)]
#[path = "decode_graph_tests.rs"]
mod tests;
