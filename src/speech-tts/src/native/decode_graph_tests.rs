//! Exact former lifetime obligations on the production Rust cache ABI.

use std::{cell::RefCell, ffi::c_void, rc::Rc};

use super::decode_graph::{
    bonsai_qt_decode_cache_begin, bonsai_qt_decode_cache_complete, bonsai_qt_decode_cache_free,
    bonsai_qt_decode_cache_new,
};

#[derive(Default)]
struct State {
    allocations: usize,
    releases: usize,
    live: usize,
    before_build_live: Vec<usize>,
    released_during_compute: bool,
    computing: bool,
}

struct Graph {
    id: usize,
    state: Rc<RefCell<State>>,
}

impl Drop for Graph {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.released_during_compute |= state.computing;
        state.releases += 1;
        state.live -= 1;
    }
}

struct Builder {
    state: Rc<RefCell<State>>,
    fail: bool,
    partial_failure: bool,
}

// SAFETY: Tests pass a unique Builder only during synchronous cache_begin.
// This model-free callback follows the real noexcept builder's transfer rule.
unsafe extern "C" fn build(data: *mut c_void) -> *mut c_void {
    // SAFETY: The helper supplies its live, exclusively borrowed Builder.
    let builder = unsafe { &mut *data.cast::<Builder>() };
    let mut state = builder.state.borrow_mut();
    let live = state.live;
    state.before_build_live.push(live);
    if builder.fail {
        return std::ptr::null_mut();
    }
    state.allocations += 1;
    state.live += 1;
    let graph = Box::new(Graph {
        id: state.allocations,
        state: Rc::clone(&builder.state),
    });
    drop(state);
    if builder.partial_failure {
        // The actual native builder also cleans partial GGML allocations
        // before returning null; no incomplete graph transfers to Rust.
        drop(graph);
        return std::ptr::null_mut();
    }
    Box::into_raw(graph).cast()
}

// SAFETY: The production Rust owner returns the one pointer transferred above,
// once, only after synchronized work is complete. No graph borrow survives.
unsafe extern "C" fn destroy(graph: *mut c_void) {
    // SAFETY: graph is the original unique Box<Graph>, released exactly once.
    drop(unsafe { Box::from_raw(graph.cast::<Graph>()) });
}

struct Owner(*mut c_void);

impl Owner {
    fn new() -> Self {
        Self(bonsai_qt_decode_cache_new(16))
    }

    fn begin(&self, tokens: i32, width: i32, builder: &mut Builder) -> *mut c_void {
        bonsai_qt_decode_cache_begin(
            self.0,
            tokens,
            width,
            (&raw mut *builder).cast(),
            Some(build),
            Some(destroy),
        )
    }

    fn complete(&self) {
        bonsai_qt_decode_cache_complete(self.0);
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        bonsai_qt_decode_cache_free(self.0);
    }
}

fn id(graph: *mut c_void) -> usize {
    assert!(!graph.is_null());
    // SAFETY: Callers inspect the initialized Graph only during its active
    // cache lease, before completion can permit replacement or destruction.
    unsafe { (*graph.cast::<Graph>()).id }
}

fn builder() -> Builder {
    Builder {
        state: Rc::default(),
        fail: false,
        partial_failure: false,
    }
}

#[test]
fn rust_owned_decode_cache_preserves_every_native_lifetime_assertion() {
    let cache = Owner::new();
    assert!(!cache.0.is_null());
    let mut builder = builder();
    let first = id(cache.begin(256, 1, &mut builder));
    cache.complete();
    assert_eq!(id(cache.begin(256, 1, &mut builder)), first);
    cache.complete();
    assert_eq!(builder.state.borrow().allocations, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    for window in (512..=4096).step_by(256) {
        assert!(!cache.begin(window, 1, &mut builder).is_null());
        cache.complete();
        let state = builder.state.borrow();
        assert_eq!(state.live, 1);
        assert_eq!(state.releases, state.allocations - 1);
    }
    assert_ne!(id(cache.begin(256, 1, &mut builder)), first);
    cache.complete();
    let before_width_change = builder.state.borrow().allocations;
    assert!(!cache.begin(256, 2, &mut builder).is_null());
    cache.complete();
    assert_eq!(builder.state.borrow().allocations, before_width_change + 1);
    builder.fail = true;
    assert!(cache.begin(1024, 1, &mut builder).is_null());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(!cache.begin(1024, 1, &mut builder).is_null());
    cache.complete();
    builder.fail = true;
    assert!(cache.begin(1024, 2, &mut builder).is_null());
    assert_eq!(builder.state.borrow().live, 0);
    assert_eq!(
        builder.state.borrow().releases,
        builder.state.borrow().allocations
    );
    builder.fail = false;
    assert!(!cache.begin(1024, 2, &mut builder).is_null());
    cache.complete();
    drop(cache);
    let state = builder.state.borrow();
    assert_eq!(state.live, 0);
    assert_eq!(state.releases, state.allocations);
    assert!(state.before_build_live.iter().all(|&live| live == 0));
    assert!(!state.released_during_compute);
}

#[test]
fn rust_owned_decode_cache_preserves_compute_lease_partial_failure_and_isolation() {
    let first = Owner::new();
    let second = Owner::new();
    let mut a = builder();
    let mut b = builder();
    assert!(!first.begin(256, 1, &mut a).is_null());
    a.state.borrow_mut().computing = true;
    assert!(first.begin(4096, 2, &mut a).is_null());
    assert!(first.begin(256, 1, &mut a).is_null());
    assert_eq!(a.state.borrow().live, 1);
    assert_eq!(a.state.borrow().releases, 0);
    assert!(!second.begin(4096, 2, &mut b).is_null());
    second.complete();
    a.state.borrow_mut().computing = false;
    first.complete();
    a.partial_failure = true;
    assert!(first.begin(512, 1, &mut a).is_null());
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(a.state.borrow().releases, a.state.borrow().allocations);
    assert_eq!(b.state.borrow().live, 1);
    a.partial_failure = false;
    assert!(!first.begin(512, 1, &mut a).is_null());
    first.complete();
    drop(first);
    drop(second);
    assert_eq!(a.state.borrow().live, 0);
    assert_eq!(b.state.borrow().live, 0);
    assert!(!a.state.borrow().released_during_compute);
}

#[test]
fn rust_owned_decode_cache_rejects_invalid_calls_without_build_or_eviction() {
    assert!(bonsai_qt_decode_cache_new(0).is_null());
    bonsai_qt_decode_cache_free(std::ptr::null_mut());
    let cache = Owner::new();
    let mut builder = builder();
    for (tokens, width) in [(0, 1), (-1, 1), (4097, 1), (256, 0), (256, -1)] {
        assert!(cache.begin(tokens, width, &mut builder).is_null());
    }
    assert_eq!(builder.state.borrow().before_build_live, []);
    assert!(!cache.begin(256, 1, &mut builder).is_null());
    cache.complete();
    let callbacks: [Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>; 2] =
        [None, Some(build)];
    for callback in callbacks {
        assert!(
            bonsai_qt_decode_cache_begin(cache.0, 256, 1, std::ptr::null_mut(), callback, None)
                .is_null()
        );
    }
    assert_eq!(builder.state.borrow().live, 1);
    assert_eq!(builder.state.borrow().releases, 0);
    drop(cache);
    assert_eq!(builder.state.borrow().live, 0);
}
