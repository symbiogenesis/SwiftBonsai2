//! Independent ownership checks against the production safe cache.
#![forbid(unsafe_code)]

use std::{cell::Cell, rc::Rc};

use super::{Cache, window_index};

struct Resource(Rc<Cell<usize>>);

impl Drop for Resource {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn selection_preserves_every_window_boundary_and_rejects_invalid_extents() {
    for window in 0..16 {
        for tokens in (window * 256 + 1)..=((window + 1) * 256) {
            assert_eq!(window_index(tokens, 16), usize::try_from(window).ok());
        }
    }
    for tokens in [i32::MIN, -1, 0, 4097, i32::MAX] {
        assert_eq!(window_index(tokens, 16), None);
    }
    assert_eq!(window_index(256, 0), None);
    assert_eq!(window_index(256, 1), Some(0));
    assert_eq!(window_index(257, 1), None);
}

#[test]
fn owned_cache_reuses_exact_geometry_and_releases_before_replacement() -> Result<(), &'static str> {
    let released = Rc::new(Cell::new(0));
    let mut cache = Cache::new(16).ok_or("valid cache geometry was rejected")?;
    for width in 1..=16 {
        let before = released.get();
        assert!(
            cache
                .begin(256, width, || {
                    assert_eq!(released.get(), usize::try_from(width - 1).ok().unwrap_or(0));
                    Some(Resource(Rc::clone(&released)))
                })
                .is_some()
        );
        assert!(cache.complete());
        assert!(cache.begin(256, width, || None).is_some());
        assert!(cache.complete());
        assert_eq!(released.get(), before + usize::from(width != 1));
    }
    assert!(!cache.complete());
    drop(cache);
    assert_eq!(released.get(), 16);
    Ok(())
}

#[test]
fn failed_build_keeps_an_empty_cache_and_active_work_prevents_eviction() -> Result<(), &'static str>
{
    let released = Rc::new(Cell::new(0));
    let mut cache = Cache::new(16).ok_or("valid cache geometry was rejected")?;
    assert!(
        cache
            .begin(256, 1, || Some(Resource(Rc::clone(&released))))
            .is_some()
    );
    assert!(cache.begin(4096, 2, || None).is_none());
    assert_eq!(released.get(), 0);
    assert!(cache.complete());
    assert!(
        cache
            .begin(4096, 2, || {
                assert_eq!(released.get(), 1);
                None
            })
            .is_none()
    );
    assert!(!cache.complete());
    assert!(
        cache
            .begin(4096, 2, || Some(Resource(Rc::clone(&released))))
            .is_some()
    );
    assert!(cache.complete());
    drop(cache);
    assert_eq!(released.get(), 2);
    Ok(())
}

#[test]
fn every_original_cached_requested_width_pair_uses_the_same_owner() -> Result<(), &'static str> {
    for cached_width in 1..=16 {
        for requested_width in 1..=16 {
            let released = Rc::new(Cell::new(0));
            let rebuilt = Cell::new(false);
            let mut cache = Cache::new(16).ok_or("valid cache geometry was rejected")?;
            assert!(
                cache
                    .begin(256, cached_width, || Some(Resource(Rc::clone(&released))))
                    .is_some()
            );
            assert!(cache.complete());
            assert!(
                cache
                    .begin(256, requested_width, || {
                        rebuilt.set(true);
                        Some(Resource(Rc::clone(&released)))
                    })
                    .is_some()
            );
            assert_eq!(rebuilt.get(), cached_width != requested_width);
            assert!(cache.complete());
            drop(cache);
            assert_eq!(
                released.get(),
                1 + usize::from(cached_width != requested_width)
            );
        }
    }
    Ok(())
}

#[test]
fn original_fresh_request_schedule_retains_exact_reuse_and_drop_counts() -> Result<(), &'static str>
{
    let released = Rc::new(Cell::new(0));
    let created = Cell::new(0);
    let mut cache = Cache::new(16).ok_or("valid cache geometry was rejected")?;
    for tokens in [256, 4096, 256, 1024, 1024, 256] {
        assert!(
            cache
                .begin(tokens, 2, || {
                    created.set(created.get() + 1);
                    Some(Resource(Rc::clone(&released)))
                })
                .is_some()
        );
        assert!(cache.complete());
    }
    assert_eq!(created.get(), 5);
    assert_eq!(released.get(), 4);
    drop(cache);
    assert_eq!(released.get(), created.get());
    Ok(())
}
