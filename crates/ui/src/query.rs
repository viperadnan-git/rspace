//! Stale-while-revalidate async resource for gpui views, over [`QueryCache`].
//! Serves cache instantly, revalidates in the background, dedupes, race-guards.

use std::collections::HashSet;
use std::fmt::Display;
use std::future::Future;
use std::hash::Hash;
use std::time::Duration;

use gpui::{Context, Window};
use rspace_core::{Lookup, QueryCache};

pub enum Status {
    Idle,
    Loading,
    Revalidating,
    Ready,
    /// Reached only when there was no cached data to fall back on.
    Error(String),
}

/// Bound to a single "current" key at a time.
pub struct Query<K, V> {
    cache: QueryCache<K, V>,
    current: Option<K>,
    data: Option<V>,
    status: Status,
    in_flight: HashSet<K>,
}

impl<K, V> Query<K, V>
where
    K: Eq + Hash + Clone + 'static,
    V: Clone + 'static,
{
    pub fn new(stale_after: Duration) -> Self {
        Self {
            cache: QueryCache::new(stale_after),
            current: None,
            data: None,
            status: Status::Idle,
            in_flight: HashSet::new(),
        }
    }

    pub fn data(&self) -> Option<&V> {
        self.data.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn is_fetching(&self) -> bool {
        self.current.as_ref().is_some_and(|k| self.in_flight.contains(k))
    }

    pub fn set_stale_after(&mut self, stale_after: Duration) {
        self.cache.set_stale_after(stale_after);
    }

    /// Mutate the current value in place (display copy and cache), e.g. to
    /// re-sort without refetching.
    pub fn update_current(&mut self, f: impl Fn(&mut V)) {
        if let Some(data) = self.data.as_mut() {
            f(data);
        }
        if let Some(key) = self.current.clone() {
            if let Some(v) = self.cache.get_mut(&key) {
                f(v);
            }
        }
    }

    /// Make `key` the current query: serve cache, fetch in the background if
    /// missing or stale. `access` recovers `&mut Self` from the view; `fetch`
    /// builds the (`'static`) future from the key.
    pub fn load<View, E, Fut>(
        &mut self,
        key: K,
        cx: &mut Context<View>,
        access: fn(&mut View) -> &mut Self,
        fetch: impl FnOnce(K) -> Fut,
    ) where
        View: 'static,
        E: Display + 'static,
        Fut: Future<Output = Result<V, E>> + 'static,
    {
        let same_key = self.current.as_ref() == Some(&key);
        self.current = Some(key.clone());
        let need_fetch = match self.cache.lookup(&key) {
            Lookup::Fresh(v) => {
                self.data = Some(v.clone());
                self.status = Status::Ready;
                false
            }
            Lookup::Stale(v) => {
                self.data = Some(v.clone());
                self.status = Status::Revalidating;
                true
            }
            // Keep a prior error visible while refetching the same key; only show
            // loading on a fresh key or first load.
            Lookup::Miss if same_key && matches!(self.status, Status::Error(_)) => true,
            Lookup::Miss => {
                self.data = None;
                self.status = Status::Loading;
                true
            }
        };
        cx.notify();
        if need_fetch {
            self.spawn_fetch(key, cx, access, fetch);
        }
    }

    /// Force a refetch of the current key, bypassing the stale gate and keeping
    /// any on-screen data.
    pub fn reload<View, E, Fut>(
        &mut self,
        cx: &mut Context<View>,
        access: fn(&mut View) -> &mut Self,
        fetch: impl FnOnce(K) -> Fut,
    ) where
        View: 'static,
        E: Display + 'static,
        Fut: Future<Output = Result<V, E>> + 'static,
    {
        let Some(key) = self.current.clone() else {
            return;
        };
        // Keep current data or a prior error visible; only show loading on a cold reload.
        if self.data.is_some() {
            self.status = Status::Revalidating;
        } else if !matches!(self.status, Status::Error(_)) {
            self.status = Status::Loading;
        }
        cx.notify();
        self.spawn_fetch(key, cx, access, fetch);
    }

    fn spawn_fetch<View, E, Fut>(
        &mut self,
        key: K,
        cx: &mut Context<View>,
        access: fn(&mut View) -> &mut Self,
        fetch: impl FnOnce(K) -> Fut,
    ) where
        View: 'static,
        E: Display + 'static,
        Fut: Future<Output = Result<V, E>> + 'static,
    {
        // Dedup: a fetch for this key is already running.
        if self.in_flight.contains(&key) {
            return;
        }
        self.in_flight.insert(key.clone());
        let fut = fetch(key.clone());

        cx.spawn(async move |view, cx| {
            let result = fut.await;
            view.update(cx, move |view, cx| {
                let q = access(view);
                q.in_flight.remove(&key);
                let is_current = q.current.as_ref() == Some(&key);
                match result {
                    Ok(value) => {
                        q.cache.insert(key.clone(), value.clone());
                        // Apply only if still current, so a slow response can't
                        // clobber a newer navigation.
                        if is_current {
                            q.data = Some(value);
                            q.status = Status::Ready;
                        }
                    }
                    Err(e) => {
                        if is_current {
                            q.status = if q.data.is_some() {
                                Status::Ready
                            } else {
                                Status::Error(e.to_string())
                            };
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Focus-gated interval poll: runs `tick` every `interval_of(view)` while the
/// window is active, ending when the view is gone. Interval is re-read each tick.
pub fn poll<View: 'static>(
    window: &Window,
    cx: &mut Context<View>,
    interval_of: fn(&View) -> Duration,
    tick: fn(&mut View, &mut Context<View>),
) {
    cx.spawn_in(window, async move |this, cx| {
        loop {
            let interval = match cx.update(|_, app| this.update(app, |v, _| interval_of(v)).ok()) {
                Ok(Some(d)) => d,
                _ => break,
            };
            cx.background_executor().timer(interval).await;
            let proceed = cx.update(|window, app| {
                let active = window.is_window_active();
                this.update(app, |view, vcx| {
                    if active {
                        tick(view, vcx);
                    }
                })
                .is_ok()
            });
            if !matches!(proceed, Ok(true)) {
                break;
            }
        }
    })
    .detach();
}
