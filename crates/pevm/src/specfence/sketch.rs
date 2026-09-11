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
    live_unfenced: AtomicUsize,
    /// `usize::MAX` = unknown.
    predicted_writer: AtomicUsize,
    /// Reader that consumed the live canary grant (`usize::MAX` = none).
    canary_tx: AtomicUsize,
}

impl Default for LocSketch {
    fn default() -> Self {
        Self {
            canaries: AtomicUsize::new(0),
            publishes: AtomicUsize::new(0),
            live_unfenced: AtomicUsize::new(0),
            predicted_writer: AtomicUsize::new(usize::MAX),
            canary_tx: AtomicUsize::new(usize::MAX),
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

/// One canary Unfenced per hot ℓ before Avoid (A2).
const CANARY_GRANT: usize = 1;
/// Clique size at which further Unfenced on unpublished ℓ is gated (A1).
const CLIQUE_UNFENCED_CAP: usize = 1;

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

    /// A6 / U6: warm-start H + templates from inter-block top-ℓ.
    /// Flip / quiet / low-confidence priors decay; they do **not** arm Wait.
    pub(crate) fn seed_from_prior(&self, tops: &[TopLocPrior], flipped: bool) {
        self.seed_from_prior_morph(tops, flipped, false);
    }

    /// U6: quiet morph (098↔599) extra-decays the prior so Fence bits do not
    /// stick across a quiet follow-on block.
    pub(crate) fn seed_from_prior_morph(
        &self,
        tops: &[TopLocPrior],
        flipped: bool,
        quiet: bool,
    ) {
        for top in tops {
            let mut decay = if flipped { 0.45 } else { 1.0 };
            if quiet {
                decay *= 0.40;
            }
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
    /// U2: spine only — do not fall back to a sticky predicted-writer from a
    /// prior incarnation that may no longer write ℓ.
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
        None
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

    /// Try to consume the single canary Unfenced grant (A2).
    /// Does **not** require H — first-wave discovery on a forming clique.
    pub(crate) fn try_canary(&self, location: MemoryLocationHash, reader: TxIdx) -> bool {
        if self.avoid_broadcast(location) {
            return false;
        }
        let e = self.locs.entry(location).or_default();
        let n = e.canaries.fetch_add(1, Ordering::Relaxed);
        if n < CANARY_GRANT {
            e.canary_tx.store(reader, Ordering::Relaxed);
            self.canary_probes.fetch_add(1, Ordering::Relaxed);
            self.note_hot(location);
            true
        } else {
            false
        }
    }

    /// Re-open the canary after the probe finished without Avoid (first-wave).
    /// Concurrent Unfenced on this ℓ stays 1; not a mass-Unfenced grant.
    pub(crate) fn reopen_canary_if_probe_done(
        &self,
        location: MemoryLocationHash,
        probe_done: bool,
    ) -> bool {
        if self.avoid_broadcast(location) || !probe_done {
            return false;
        }
        let e = self.locs.entry(location).or_default();
        e.canaries.store(0, Ordering::Relaxed);
        e.canary_tx.store(usize::MAX, Ordering::Relaxed);
        true
    }

    pub(crate) fn canary_tx(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        self.locs.get(&location).and_then(|e| {
            let t = e.canary_tx.load(Ordering::Relaxed);
            if t == usize::MAX {
                None
            } else {
                Some(t)
            }
        })
    }

    /// First-wave probe already consumed (serial lane, even before H/Avoid).
    pub(crate) fn canary_taken(&self, location: MemoryLocationHash) -> bool {
        self.locs
            .get(&location)
            .is_some_and(|e| e.canaries.load(Ordering::Relaxed) >= CANARY_GRANT)
    }

    pub(crate) fn note_unfenced(&self, location: MemoryLocationHash) {
        self.locs
            .entry(location)
            .or_default()
            .live_unfenced
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A1: mass Unfenced on unpublished clique is gated after the canary.
    pub(crate) fn clique_gated(&self, location: MemoryLocationHash) -> bool {
        if !self.in_h(location)
            && !self.avoid_broadcast(location)
            && !self.canary_taken(location)
        {
            return false;
        }
        let unfenced = self
            .locs
            .get(&location)
            .map(|e| e.live_unfenced.load(Ordering::Relaxed))
            .unwrap_or(0);
        if unfenced >= CLIQUE_UNFENCED_CAP || self.canary_taken(location) {
            self.clique_gates.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// A4: no H, no Avoid, no live template, no canary taken → Unfenced.
    pub(crate) fn independence_certified(&self, location: MemoryLocationHash) -> bool {
        !self.in_serial_lane(location)
    }

    /// Hot-record serialization lane (A4 contention-split).
    /// Canary-taken means a second reader of ℓ is no longer independent.
    pub(crate) fn in_serial_lane(&self, location: MemoryLocationHash) -> bool {
        self.in_h(location)
            || self.avoid_broadcast(location)
            || self.template_live(location)
            || self.canary_taken(location)
    }

    /// Closest unfinished lower writer on the spine (Fence target, not reader-1).
    /// U2: no predicted-writer fallback (repair incarnations must not stick).
    pub(crate) fn next_unfinished_writer_before(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
        is_done: impl Fn(TxIdx) -> bool,
    ) -> Option<TxIdx> {
        self.unfinished_writers_before(location, reader, is_done)
            .into_iter()
            .next_back()
    }

    /// S4: every unfinished lower writer on this ℓ's spine (not only the tip).
    pub(crate) fn unfinished_writers_before(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
        is_done: impl Fn(TxIdx) -> bool,
    ) -> Vec<TxIdx> {
        if let Some(e) = self.spines.get(&location) {
            let v = e.lock().unwrap();
            return v
                .iter()
                .copied()
                .filter(|&w| w < reader && !is_done(w))
                .collect();
        }
        Vec::new()
    }

    /// U2: drop a writer that no longer publishes ℓ (suffix invalidate / abort).
    /// Refreshes `predicted_writer` from the remaining spine — no min-sticky ghost.
    pub(crate) fn forget_writer(&self, location: MemoryLocationHash, writer: TxIdx) {
        if let Some(e) = self.spines.get(&location) {
            let mut v = e.lock().unwrap();
            if let Ok(i) = v.binary_search(&writer) {
                v.remove(i);
            }
            let next = v.first().copied().unwrap_or(usize::MAX);
            drop(v);
            if let Some(loc) = self.locs.get(&location) {
                loc.predicted_writer.store(next, Ordering::Relaxed);
            }
        } else if let Some(loc) = self.locs.get(&location) {
            let _ = loc.predicted_writer.compare_exchange(
                writer,
                usize::MAX,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// U6: revoke warm-seeded H/templates that have no live Avoid.
    /// Quiet morph (098↔599) must not keep prior Fences; live Avoid stays.
    pub(crate) fn revoke_prior_fences_if_quiet(&self, quiet: bool) -> usize {
        if !quiet {
            return 0;
        }
        let mut n = 0usize;
        for loc in self.warm_seeded.iter() {
            let loc = *loc;
            if self.avoid_broadcast(loc) {
                continue;
            }
            if self.templates.remove(&loc).is_some() {
                n += 1;
            }
            if self.hot.remove(&loc).is_some() {
                self.hot_size.fetch_sub(1, Ordering::Relaxed);
                n += 1;
            }
            self.decay_events.fetch_add(1, Ordering::Relaxed);
        }
        n
    }

    /// Essential unpublished anti-dep: H / Avoid / template / known writer.
    pub(crate) fn essential_antidep(
        &self,
        location: MemoryLocationHash,
        writer_known: bool,
        prior_ws: bool,
        force_prefix: bool,
    ) -> bool {
        if force_prefix || self.avoid_broadcast(location) {
            // Repair prefix / Avoid are essential even before the writer is resolved.
            // Hang-freedom is serial-lane admission, not Unfenced.
            return true;
        }
        if !writer_known {
            return false;
        }
        self.in_h(location) || prior_ws || self.template_live(location)
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
        assert!(s.try_canary(1, 3));
        assert_eq!(s.canary_tx(1), Some(3));
        assert!(s.canary_taken(1));
        assert!(s.in_serial_lane(1));
        assert!(!s.independence_certified(1));
        assert!(!s.try_canary(1, 4));
        assert!(s.broadcast_avoid(1, 0));
        assert!(s.avoid_broadcast(1));
        assert!(!s.try_canary(1, 5));
        s.note_unfenced(1);
        assert!(s.clique_gated(1));
    }

    #[test]
    fn canary_without_h_serializes_second_reader() {
        let s = HotSketch::new();
        assert!(s.try_canary(9, 2));
        assert!(s.in_h(9), "canary promotes ℓ into H");
        assert!(s.canary_taken(9));
        assert!(!s.independence_certified(9));
        assert!(s.clique_gated(9));
        assert!(!s.try_canary(9, 8));
    }

    #[test]
    fn reopen_canary_after_probe() {
        let s = HotSketch::new();
        assert!(s.try_canary(2, 1));
        assert!(!s.try_canary(2, 4));
        assert!(s.reopen_canary_if_probe_done(2, true));
        assert!(s.try_canary(2, 4));
        assert_eq!(s.canary_tx(2), Some(4));
        assert!(!s.reopen_canary_if_probe_done(2, false));
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
        assert_eq!(
            s.next_unfinished_writer_before(1, 15, |w| w == 12),
            Some(7),
            "skip finished 12, take 7"
        );
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

    #[test]
    fn force_prefix_essential_without_writer() {
        let s = HotSketch::new();
        assert!(
            s.essential_antidep(1, false, false, true),
            "force_prefix ∧ writer=None is still essential (serial-lane Fence)"
        );
    }

    #[test]
    fn avoid_essential_without_writer() {
        let s = HotSketch::new();
        assert!(s.broadcast_avoid(2, 0));
        assert!(
            s.essential_antidep(2, false, false, false),
            "Avoid ∧ writer=None is still essential"
        );
    }

    #[test]
    fn forget_writer_drops_predicted_stickiness() {
        let s = HotSketch::new();
        s.push_spine(1, 3);
        s.push_spine(1, 9);
        assert_eq!(s.next_writer_before(1, 10), Some(9));
        s.forget_writer(1, 9);
        assert_eq!(
            s.next_writer_before(1, 10),
            Some(3),
            "U2: repair must not keep a dropped writer as the predicted tip"
        );
        s.forget_writer(1, 3);
        assert_eq!(s.next_writer_before(1, 10), None);
        assert_eq!(s.predicted_writer(1), None);
    }

    #[test]
    fn multi_spine_unfinished_all_writers() {
        let s = HotSketch::new();
        s.push_spine(7, 2);
        s.push_spine(7, 5);
        s.push_spine(7, 11);
        let live = s.unfinished_writers_before(7, 12, |w| w == 5);
        assert_eq!(live, vec![2, 11], "S4: every unfinished writer on ℓ");
    }

    #[test]
    fn quiet_revoke_drops_warm_fence_keeps_avoid() {
        let s = HotSketch::new();
        s.seed_from_prior_morph(
            &[TopLocPrior {
                location: 4,
                fanout_ema: 16.0,
                abort_rate: 0.05,
                chain_len_ema: 3.0,
            }],
            true,
            true,
        );
        // quiet × flip decay of 0.85 → 0.85*0.45*0.40 = 0.153 < CONF_LIVE
        assert!(
            !s.in_h(4),
            "U6: quiet+flip must not seed a Fence prior"
        );
        s.seed_from_prior(
            &[TopLocPrior {
                location: 8,
                fanout_ema: 16.0,
                abort_rate: 0.05,
                chain_len_ema: 3.0,
            }],
            false,
        );
        assert!(s.in_h(8));
        assert!(s.broadcast_avoid(8, 1));
        s.seed_from_prior(
            &[TopLocPrior {
                location: 9,
                fanout_ema: 16.0,
                abort_rate: 0.05,
                chain_len_ema: 3.0,
            }],
            false,
        );
        assert!(s.in_h(9));
        let n = s.revoke_prior_fences_if_quiet(true);
        assert!(n >= 1, "warm ℓ without Avoid revoked: {n}");
        assert!(!s.in_h(9));
        assert!(s.in_h(8), "live Avoid is not a quiet-revoke target");
        assert!(s.avoid_broadcast(8));
    }
}
