//! Ahead-of-time dependency sketch + ordered admission (A1/D1) and
//! first-wave Avoid + inter warm-start (A2/A6).
//!
//! Not Storm/Quiet morph-as-protocol. Morph flip only **decays** template
//! confidence (A6). SoftWait Soft is never armed from a sketch.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::learner::TopLocPrior;

/// Live confidence floor — below this the template is not a Wait/H prior (A6).
const CONF_LIVE: f64 = 0.20;

/// Predicted wr spine on a hot location (order = preset tx index).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChainTemplate {
    pub location: MemoryLocationHash,
    pub predicted_fanout: f64,
    pub predicted_chain_len: f64,
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
    /// Ordered writer indices per ℓ (version spine). Not fanout stubs.
    spines: DashMap<MemoryLocationHash, Mutex<Vec<TxIdx>>, BuildIdentityHasher>,
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
            if conf < CONF_LIVE {
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
                    predicted_chain_len: top.chain_len_ema.max(1.0),
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
        drop(e);
        self.push_spine(location, writer);
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
    /// Keep the **lowest** index — first-to-publish later txs must not become
    /// the WaitFor target for earlier readers (preset-order inversion).
    pub(crate) fn note_writer(&self, location: MemoryLocationHash, writer: TxIdx) {
        let e = self.locs.entry(location).or_default();
        loop {
            let cur = e.predicted_writer.load(Ordering::Relaxed);
            if writer >= cur {
                break;
            }
            if e.predicted_writer
                .compare_exchange_weak(cur, writer, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    pub(crate) fn predicted_writer(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        if let Some(e) = self.spines.get(&location) {
            let v = e.lock().unwrap();
            if let Some(&w) = v.first() {
                return Some(w);
            }
        }
        self.locs.get(&location).and_then(|e| e.predicted_writer())
    }

    /// Push a writer onto the ordered spine (sorted unique).
    pub(crate) fn push_spine(&self, location: MemoryLocationHash, writer: TxIdx) {
        self.note_writer(location, writer);
        let e = self.spines.entry(location).or_insert_with(|| Mutex::new(Vec::new()));
        let mut v = e.lock().unwrap();
        if let Err(i) = v.binary_search(&writer) {
            v.insert(i, writer);
        }
    }

    /// Closest lower writer on the version spine (A1 ordered admission).
    pub(crate) fn next_writer_before(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
    ) -> Option<TxIdx> {
        if let Some(e) = self.spines.get(&location) {
            let v = e.lock().unwrap();
            if let Some(&w) = v.iter().rev().find(|&&w| w < reader) {
                return Some(w);
            }
        }
        self.locs
            .get(&location)
            .and_then(|e| e.predicted_writer())
            .filter(|&w| w < reader)
    }

    pub(crate) fn spine_len(&self, location: MemoryLocationHash) -> usize {
        self.spines
            .get(&location)
            .map(|e| e.lock().unwrap().len())
            .unwrap_or(0)
    }

    /// A6: template is a live Wait/H prior only while confidence holds.
    pub(crate) fn template_live(&self, location: MemoryLocationHash) -> bool {
        self.templates
            .get(&location)
            .is_some_and(|t| t.confidence >= CONF_LIVE)
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

    /// A4: no H, no Avoid, no live template → independence-certified Spec.
    pub(crate) fn independence_certified(&self, location: MemoryLocationHash) -> bool {
        !self.in_serial_lane(location)
    }

    /// Hot-record serialization lane (A4 contention-split).
    pub(crate) fn in_serial_lane(&self, location: MemoryLocationHash) -> bool {
        self.in_h(location) || self.avoid_broadcast(location) || self.template_live(location)
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
            || self.template_live(location)
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
    /// Confidence drop below [`CONF_LIVE`] removes H + template (live, not stub).
    pub(crate) fn decay_warm_failures(&self, abort_rate: impl Fn(MemoryLocationHash) -> f64) {
        for loc in self.warm_seeded.iter() {
            let loc = *loc;
            if abort_rate(loc) >= 0.40 {
                let drop = if let Some(mut t) = self.templates.get_mut(&loc) {
                    t.confidence *= 0.35;
                    t.confidence < CONF_LIVE
                } else {
                    true
                };
                self.decay_events.fetch_add(1, Ordering::Relaxed);
                if drop {
                    self.templates.remove(&loc);
                    if self.hot.remove(&loc).is_some() {
                        self.hot_size.fetch_sub(1, Ordering::Relaxed);
                    }
                }
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
                chain_len_ema: 4.0,
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
    fn spine_keeps_lowest_writer() {
        let s = HotSketch::new();
        s.broadcast_avoid(1, 20);
        s.push_spine(1, 7);
        s.push_spine(1, 12);
        assert_eq!(s.predicted_writer(1), Some(7));
    }

    #[test]
    fn ordered_spine_next_writer() {
        let s = HotSketch::new();
        s.push_spine(1, 20);
        s.push_spine(1, 7);
        s.push_spine(1, 12);
        assert_eq!(s.next_writer_before(1, 15), Some(12));
        assert_eq!(s.next_writer_before(1, 8), Some(7));
        assert_eq!(s.next_writer_before(1, 7), None);
        assert_eq!(s.spine_len(1), 3);
        s.note_hot(1);
        assert!(s.in_serial_lane(1));
        assert!(!s.independence_certified(1));
    }

    #[test]
    fn decay_drops_live_template() {
        let s = HotSketch::new();
        s.seed_from_prior(
            &[TopLocPrior {
                location: 4,
                fanout_ema: 4.0,
                abort_rate: 0.1,
                chain_len_ema: 6.0,
            }],
            false,
        );
        assert!(s.template_live(4));
        s.decay_warm_failures(|_| 0.9);
        assert!(!s.template_live(4));
        assert!(!s.in_h(4));
        assert!(s.independence_certified(4));
    }

    #[test]
    fn flip_decays_low_confidence() {
        let s = HotSketch::new();
        s.seed_from_prior(
            &[TopLocPrior {
                location: 3,
                fanout_ema: 4.0,
                abort_rate: 0.5,
                chain_len_ema: 2.0,
            }],
            true,
        );
        // abort 0.5 * flip decay → skipped
        assert!(!s.in_h(3));
        assert!(s.decay_events() >= 1);
    }
}
