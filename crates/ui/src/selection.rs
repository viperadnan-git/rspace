//! A reusable multi-selection: a set of item ids plus a shift-range anchor.
//! Stores ids (not indices) so the set survives reordering/refresh; the owner
//! supplies the current render order for range/all. Used by the file list and the
//! Tasks panel — each pairs it with its own cursor/scroll concerns.

use std::collections::HashSet;
use std::hash::Hash;

pub(crate) struct Selection<Id> {
    set: HashSet<Id>,
    /// Fixed end of a shift-range; follows the last plain click/toggle.
    anchor: Option<Id>,
}

impl<Id: Eq + Hash + Clone> Selection<Id> {
    pub(crate) fn new() -> Self {
        Self { set: HashSet::new(), anchor: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.set.len()
    }

    pub(crate) fn contains(&self, id: &Id) -> bool {
        self.set.contains(id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Id> {
        self.set.iter()
    }

    pub(crate) fn snapshot(&self) -> &HashSet<Id> {
        &self.set
    }

    pub(crate) fn select_only(&mut self, id: Id) {
        self.set.clear();
        self.set.insert(id.clone());
        self.anchor = Some(id);
    }

    pub(crate) fn toggle(&mut self, id: Id) {
        if !self.set.remove(&id) {
            self.set.insert(id.clone());
        }
        self.anchor = Some(id);
    }

    pub(crate) fn clear(&mut self) {
        self.set.clear();
        self.anchor = None;
    }

    /// Shift-range from the anchor to `id` within `ordered`; a single select when
    /// either endpoint is missing. The anchor stays put so a range can be widened.
    pub(crate) fn range_to(&mut self, ordered: &[Id], id: Id) {
        let anchor = self.anchor.clone().unwrap_or_else(|| id.clone());
        match (ordered.iter().position(|x| *x == anchor), ordered.iter().position(|x| *x == id)) {
            (Some(a), Some(b)) => {
                let (lo, hi) = (a.min(b), a.max(b));
                self.set = ordered[lo..=hi].iter().cloned().collect();
            }
            _ => self.select_only(id),
        }
    }

    /// Select every item, anchoring on the last (Select-all).
    pub(crate) fn all(&mut self, ordered: &[Id]) {
        self.set = ordered.iter().cloned().collect();
        self.anchor = ordered.last().cloned();
    }

    /// Replace the set wholesale, leaving the anchor (rubber-band rebuilds the set
    /// each move from a fixed base).
    pub(crate) fn set_to(&mut self, set: HashSet<Id>) {
        self.set = set;
    }

    /// Drop ids that no longer pass `keep` (e.g. after a refresh), clearing a
    /// stale anchor too.
    pub(crate) fn retain(&mut self, keep: impl Fn(&Id) -> bool) {
        self.set.retain(|id| keep(id));
        if self.anchor.as_ref().is_some_and(|a| !keep(a)) {
            self.anchor = None;
        }
    }
}
