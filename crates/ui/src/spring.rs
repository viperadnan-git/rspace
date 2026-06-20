//! Spring-loaded dwell state, shared by the tab strip, breadcrumb, and sidebar:
//! a drag hovering a target for `SPRING_LOAD_MS` activates it. This holds the
//! tracked target plus a generation counter so a stale activation timer no-ops.
//! Each call site keeps its own spawn + activation — they differ in whether they
//! need a `Window` and in what "activate" does.

pub(crate) struct SpringLoad<T> {
    pending: Option<T>,
    generation: u64,
}

impl<T: PartialEq> SpringLoad<T> {
    pub(crate) fn new() -> Self {
        Self { pending: None, generation: 0 }
    }

    /// Begin a dwell on `key`. Returns the generation to capture in the timer, or
    /// `None` if a dwell on this exact key is already pending (don't re-arm).
    pub(crate) fn arm(&mut self, key: T) -> Option<u64> {
        if self.pending.as_ref() == Some(&key) {
            return None;
        }
        self.pending = Some(key);
        self.generation += 1;
        Some(self.generation)
    }

    /// Whether the captured `(generation, key)` is still the live dwell — call
    /// when the timer fires, before activating.
    pub(crate) fn live(&self, generation: u64, key: &T) -> bool {
        self.generation == generation && self.pending.as_ref() == Some(key)
    }

    pub(crate) fn is_pending(&self, key: &T) -> bool {
        self.pending.as_ref() == Some(key)
    }

    pub(crate) fn clear(&mut self) {
        self.pending = None;
    }
}
