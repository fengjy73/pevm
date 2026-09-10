//! Ahead-of-time dependency sketch + ordered admission (A1/D1) and
//! first-wave Avoid + inter warm-start (A2/A6).
//!
//! Not Storm/Quiet morph-as-protocol. Morph flip only **decays** template
//! confidence (A6). SoftWait Soft is never armed from a sketch.

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::learner::TopLocPrior;

/// Predicted wr spine on a hot location.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChainTemplate {
    pub location: MemoryLocationHash,
    pub predicted_fanout: f64,
    pub confidence: f64,
}

/// Per-location first-wave / admission state.
#[derive(Debug)]
struct LocSketch {
    canaries: AtomicUsize,
    publishes: AtomicUsize,
    live_specs: AtomicUsize,
    /// `usize::MAX` = unknown.
    predicted_writer: AtomicUsize,
}

impl Default for LocSketch {
    fn default() -> Self {
        Self {
            canaries: AtomicUsize::new(0),
            publishes: AtomicUsize::new(0),
            live_specs: AtomicUsize::new(0),
            predicted_writer: AtomicUsize::new(usize::MAX),
        }
    }
}

impl LocSketch {
    fn predicted_writer(&self) -> Option<TxIdx> {
        let w = self.predicted_writer.load(Ordering::Relaxed);
        if w == usize::MAX {
            None
        } else {
            Some(w)
        }
    }
}

/// One canary Spec per hot ℓ before Avoid (A2).
const CANARY_GRANT: usize = 1;
/// Clique size at which further Spec on unpublished ℓ is gated (A1).
const CLIQUE_SPEC_CAP: usize = 1;

/// Ahead sketch: H + chain templates + Avoid + admission grants.
#[derive(Debug, Default)]
pub(crate) struct HotSketch {
    hot: DashSet<MemoryLocationHash, BuildIdentityHasher>,
    templates: DashMap<MemoryLocationHash, ChainTemplate, BuildIdentityHasher>,
    locs: DashMap<MemoryLocationHash, LocSketch, BuildIdentityHasher>,
    avoid: DashMap<MemoryLocationHash, (), BuildIdentityHasher>,
    warm_seeded: DashSet<MemoryLocationHash, BuildIdentityHasher>,
    hot_size: AtomicUsize,
    avoid_broadcasts: AtomicUsize,
    canary_probes: AtomicUsize,
    clique_gates: AtomicUsize,
    decay_events: AtomicUsize,
}

impl HotSketch {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A6: warm-start H + templates from inter-block top-ℓ.
    /// Flip / low-confidence priors decay; they do **not** arm Wait.
    pub(crate) fn seed_from_prior(&self, tops: &[TopLocPrior], flipped: bool) {
        for top in tops {
            let decay = if flipped { 0.45 } else { 1.0 };
            let conf = if top.abort_rate >= 0.40 {
                0.25 * decay
            } else if top.fanout_ema >= 8.0 {
                0.85 * decay
            } else {
                0.50 * decay
            };
            if conf < 0.20 {
                self.decay_events.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.hot.insert(top.location) {
                self.hot_size.fetch_add(1, Ordering::Relaxed);
            }
            self.warm_seeded.insert(top.location);
            self.templates.insert(
                top.location,
                ChainTemplate {
                    location: top.location,
                    predicted_fanout: top.fanout_ema,
                    confidence: conf,
                },
            );
        }
    }

    /// Live promote into H (first-wave, not morph mode).
    pub(crate) fn note_hot(&self, location: MemoryLocationHash) {
        if self.hot.insert(location) {
            self.hot_size.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn in_h(&self, location: MemoryLocationHash) -> bool {
        self.hot.contains(&location)
    }

    /// A2: first confirmed publish → Avoid. Returns true if newly broadcast.
    pub(crate) fn broadcast_avoid(&self, location: MemoryLocationHash, writer: TxIdx) -> bool {
        self.note_hot(location);
        let e = self.locs.entry(location).or_default();
        e.publishes.fetch_add(1, Ordering::Relaxed);
        let _ = e.predicted_writer.compare_exchange(
            usize::MAX,
            writer,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        drop(e);
        if self.avoid.insert(location, ()).is_none() {
            self.avoid_broadcasts.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    #[inline]
    pub(crate) fn avoid_broadcast(&self, location: MemoryLocationHash) -> bool {
        self.avoid.contains_key(&location)
    }

    /// Observe a writer identity for the spine (admission).
    pub(crate) fn note_writer(&self, location: MemoryLocationHash, writer: TxIdx) {
        let e = self.locs.entry(location).or_default();
        let _ = e.predicted_writer.compare_exchange(
            usize::MAX,
            writer,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    pub(crate) fn predicted_writer(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        self.locs.get(&location).and_then(|e| e.predicted_writer())
    }

    /// Try to consume the single canary Spec grant (A2).
    pub(crate) fn try_canary(&self, location: MemoryLocationHash) -> bool {
        if self.avoid_broadcast(location) {
            return false;
        }
        let e = self.locs.entry(location).or_default();
        let n = e.canaries.fetch_add(1, Ordering::Relaxed);
        if n < CANARY_GRANT {
            self.canary_probes.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub(crate) fn note_spec(&self, location: MemoryLocationHash) {
        self.locs
            .entry(location)
            .or_default()
            .live_specs
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A1: mass Spec on unpublished clique is gated after the canary.
    pub(crate) fn clique_gated(&self, location: MemoryLocationHash) -> bool {
        if !self.in_h(location) && !self.avoid_broadcast(location) {
            return false;
        }
        let specs = self
            .locs
            .get(&location)
            .map(|e| e.live_specs.load(Ordering::Relaxed))
            .unwrap_or(0);
        if specs >= CLIQUE_SPEC_CAP {
            self.clique_gates.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// A4: no H, no Avoid, no template → independence-certified Spec.
    pub(crate) fn independence_certified(&self, location: MemoryLocationHash) -> bool {
        !self.in_h(location)
            && !self.avoid_broadcast(location)
            && !self.templates.contains_key(&location)
    }

    /// Essential unpublished anti-dep: H / Avoid / template / known writer.
    pub(crate) fn essential_antidep(
        &self,
        location: MemoryLocationHash,
        writer_known: bool,
        prior_ws: bool,
        force_prefix: bool,
    ) -> bool {
        if force_prefix {
            return writer_known;
        }
        if !writer_known {
            return false;
        }
        self.in_h(location)
            || self.avoid_broadcast(location)
            || prior_ws
            || self.templates.contains_key(&location)
    }

    pub(crate) fn hot_size(&self) -> usize {
        self.hot_size.load(Ordering::Relaxed)
    }

    pub(crate) fn avoid_broadcasts(&self) -> usize {
        self.avoid_broadcasts.load(Ordering::Relaxed)
    }

    pub(crate) fn canary_probes(&self) -> usize {
        self.canary_probes.load(Ordering::Relaxed)
    }

    pub(crate) fn clique_gates(&self) -> usize {
        self.clique_gates.load(Ordering::Relaxed)
    }

    pub(crate) fn decay_events(&self) -> usize {
        self.decay_events.load(Ordering::Relaxed)
    }

    /// A6: decay warm-seeded ℓ that aborted heavily this block (warm≥cold fail).
    pub(crate) fn decay_warm_failures(&self, abort_rate: impl Fn(MemoryLocationHash) -> f64) {
        for loc in self.warm_seeded.iter() {
            let loc = *loc;
            if abort_rate(loc) >= 0.40 {
                if let Some(mut t) = self.templates.get_mut(&loc) {
                    t.confidence *= 0.35;
                }
                self.decay_events.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_and_independence() {
        let s = HotSketch::new();
        s.seed_from_prior(
            &[TopLocPrior {
                location: 9,
                fanout_ema: 32.0,
                abort_rate: 0.1,
            }],
            false,
        );
        assert!(s.in_h(9));
        assert!(!s.independence_certified(9));
        assert!(s.independence_certified(99));
    }

    #[test]
    fn canary_then_avoid_then_gate() {
        let s = HotSketch::new();
        s.note_hot(1);
        assert!(s.try_canary(1));
        assert!(!s.try_canary(1));
        assert!(s.broadcast_avoid(1, 0));
        assert!(s.avoid_broadcast(1));
        assert!(!s.try_canary(1));
        s.note_spec(1);
        assert!(s.clique_gated(1));
    }

    #[test]
    fn flip_decays_low_confidence() {
        let s = HotSketch::new();
        s.seed_from_prior(
            &[TopLocPrior {
                location: 3,
                fanout_ema: 4.0,
                abort_rate: 0.5,
            }],
            true,
        );
        // abort 0.5 * flip decay → skipped
        assert!(!s.in_h(3));
        assert!(s.decay_events() >= 1);
    }
}
