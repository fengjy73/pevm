//! Ahead-of-time dependency sketch + ordered admission (A1/D1) and
//! first-wave Avoid + inter warm-start (A2/A6).
//!
//! Not Storm/Quiet morph-as-protocol. Morph flip only **decays** template
//! confidence (A6). SoftWait Soft is never armed from a sketch.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use rustc_hash::FxBuildHasher;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx, TxIncarnation};

use super::edge::access_k_class;
use super::learner::TopLocPrior;

/// Done→Data residual Bind install. Region SoT: a Fenced ℓ always has a
/// residual version (last committed Data, or Storage after writer Done∅Data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResidualBind {
    /// Last committed MV Data (may predate the Done writer).
    Data {
        tx_idx: TxIdx,
        tx_incarnation: TxIncarnation,
    },
    /// Writer Finished without readable Data — storage origin is the Fence.
    Storage { writer: TxIdx },
}

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
    /// One reopen per ℓ per block (first-wave, not a canary mill).
    canary_reopened: AtomicUsize,
}

impl Default for LocSketch {
    fn default() -> Self {
        Self {
            canaries: AtomicUsize::new(0),
            publishes: AtomicUsize::new(0),
            live_unfenced: AtomicUsize::new(0),
            predicted_writer: AtomicUsize::new(usize::MAX),
            canary_tx: AtomicUsize::new(usize::MAX),
            canary_reopened: AtomicUsize::new(0),
        }
    }
}

impl LocSketch {
    fn predicted_writer(&self) -> Option<TxIdx> {
        let w = self.predicted_writer.load(Ordering::Relaxed);
        if w == usize::MAX { None } else { Some(w) }
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
    /// PredictedEssential access-class set: (\(ℓ\), \(k_{\mathrm{class}}\)).
    /// First-wave / prior template — **not** a per-ℓ sticky Wait for all \(k\).
    access_class: DashMap<(MemoryLocationHash, u8), (), FxBuildHasher>,
    /// Done→Data residual per ℓ (Bind SoT when writer is Done∅Data).
    residuals: DashMap<MemoryLocationHash, ResidualBind, BuildIdentityHasher>,
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
    pub(crate) fn seed_from_prior_morph(&self, tops: &[TopLocPrior], flipped: bool, quiet: bool) {
        for top in tops {
            let mut decay = if flipped { 0.45 } else { 1.0 };
            if quiet {
                // Quiet follow-on: never plant H (quiet→fan_out leak), including
                // high-abort leftovers from a prior fan_out block.
                self.decay_events.fetch_add(1, Ordering::Relaxed);
                continue;
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
            // Serial-lane access class from abort-derived k_template only.
            // Live PredictedEssential is seeded on the learner (quiet skips).
            if top.k_template > 0 {
                self.mark_access_class(top.location, top.k_template);
            }
        }
    }

    /// Mark PredictedEssential access class (\(ℓ, k_{\mathrm{class}}\)).
    pub(crate) fn mark_access_class(&self, location: MemoryLocationHash, k: u32) {
        if k == 0 {
            return;
        }
        self.access_class.insert((location, access_k_class(k)), ());
    }

    /// PredictedEssential(\(ℓ, k\)) for **this** access — not flatten(\(ℓ\)).
    #[inline]
    pub(crate) fn access_class_predicted(&self, location: MemoryLocationHash, k: u32) -> bool {
        self.access_class
            .contains_key(&(location, access_k_class(k)))
    }

    pub(crate) fn has_access_class(&self, location: MemoryLocationHash) -> bool {
        self.access_class.iter().any(|e| e.key().0 == location)
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
        // Publish installs Data residual (incarnation 0 until Bind refreshes).
        self.install_data_residual(location, writer, 0);
        if self.avoid.insert(location, ()).is_none() {
            self.avoid_broadcasts.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Install last committed Data as the Region residual (Done→Bind SoT).
    pub(crate) fn install_data_residual(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
        tx_incarnation: TxIncarnation,
    ) {
        self.residuals.insert(
            location,
            ResidualBind::Data {
                tx_idx,
                tx_incarnation,
            },
        );
    }

    /// Writer Done without Data: keep last Data residual, else Storage residual.
    pub(crate) fn install_done_residual(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
    ) -> ResidualBind {
        if let Some(e) = self.residuals.get(&location) {
            return *e;
        }
        let r = ResidualBind::Storage { writer };
        self.residuals.insert(location, r);
        r
    }

    #[inline]
    pub(crate) fn residual_bind(&self, location: MemoryLocationHash) -> Option<ResidualBind> {
        self.residuals.get(&location).map(|e| *e)
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
        let e = self
            .spines
            .entry(location)
            .or_insert_with(|| Mutex::new(Vec::new()));
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
        if e.canary_reopened.swap(1, Ordering::Relaxed) != 0 {
            return false;
        }
        e.canaries.store(0, Ordering::Relaxed);
        e.canary_tx.store(usize::MAX, Ordering::Relaxed);
        true
    }

    pub(crate) fn canary_tx(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        self.locs.get(&location).and_then(|e| {
            let t = e.canary_tx.load(Ordering::Relaxed);
            if t == usize::MAX { None } else { Some(t) }
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
        if !self.in_h(location) && !self.avoid_broadcast(location) && !self.canary_taken(location) {
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

    /// A4: no PredictedEssential access-class on \(ℓ\) → Unfenced≡OCC.
    /// H / canary / location-wide Avoid are **not** independence keys.
    pub(crate) fn independence_certified(&self, location: MemoryLocationHash) -> bool {
        !self.has_access_class(location)
    }

    /// Region-access serial lane: PredictedEssential class on \(ℓ\).
    /// H / canary / flatten-Avoid are observe-only and do not enter this.
    pub(crate) fn in_serial_lane(&self, location: MemoryLocationHash) -> bool {
        self.has_access_class(location)
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

    /// Ready writers on every live (Avoid / H / template) spine before `reader`.
    /// Scheduler law: ready-set ⊆ Region unfinished spine (secondary ℓs, not
    /// only the star). PreferAdmit is this set, not a counter on one access.
    pub(crate) fn ready_spine_writers(
        &self,
        reader: TxIdx,
        is_ready: impl Fn(TxIdx) -> bool,
        is_done: impl Fn(TxIdx) -> bool,
    ) -> Vec<TxIdx> {
        let mut out = Vec::new();
        // PredictedEssential access-class spines (not location-wide Avoid).
        // Bounded (per-access full walk was a wall tax).
        let mut seen = 0usize;
        for loc_r in self.access_class.iter() {
            if seen >= 8 {
                break;
            }
            let loc = loc_r.key().0;
            seen += 1;
            let Some(s) = self.spines.get(&loc) else {
                continue;
            };
            let v = s.lock().unwrap();
            for &w in v.iter() {
                if w < reader && !is_done(w) && is_ready(w) {
                    out.push(w);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
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

    /// Legacy location-level probe — **not** the live Avoid key.
    /// Observe / serial-lane probe — **not** the live Avoid key.
    /// Live PE gate is `LiveLearner::predicted_essential`. `force_prefix` is
    /// ignored (exclude set). H / prior_ws / template are observe-only.
    pub(crate) fn essential_antidep(
        &self,
        location: MemoryLocationHash,
        writer_known: bool,
        prior_ws: bool,
        force_prefix: bool,
    ) -> bool {
        let _ = (writer_known, prior_ws, force_prefix);
        self.has_access_class(location)
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
                k_template: 6,
            }],
            false,
        );
        assert!(s.in_h(9));
        assert!(s.access_class_predicted(9, 6));
        assert!(!s.independence_certified(9));
        assert!(s.independence_certified(99));
        assert!(
            !s.access_class_predicted(9, 12),
            "k=12 is a different class — mixed verbs"
        );
    }

    #[test]
    fn canary_then_avoid_then_gate() {
        let s = HotSketch::new();
        s.note_hot(1);
        assert!(s.try_canary(1, 3));
        assert_eq!(s.canary_tx(1), Some(3));
        assert!(s.canary_taken(1));
        // Canary is not a live Avoid key — independence holds until access-class.
        assert!(!s.in_serial_lane(1));
        assert!(s.independence_certified(1));
        s.mark_access_class(1, 6);
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
        assert!(s.in_h(9), "canary still records H as observe/prior");
        assert!(s.canary_taken(9));
        assert!(
            s.independence_certified(9),
            "canary must not key independence / Avoid"
        );
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
        assert!(
            !s.reopen_canary_if_probe_done(2, true),
            "one reopen per ℓ per block"
        );
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
        assert!(
            !s.in_serial_lane(1),
            "H membership is observe-only, not a serial-lane key"
        );
        s.mark_access_class(1, 6);
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
                k_template: 0,
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
                k_template: 0,
            }],
            true,
        );
        // abort 0.5 * flip decay → skipped
        assert!(!s.in_h(3));
        assert!(s.decay_events() >= 1);
    }

    #[test]
    fn force_prefix_is_not_essential() {
        let s = HotSketch::new();
        assert!(
            !s.essential_antidep(1, false, false, true),
            "force_prefix is excluded from Avoid π"
        );
    }

    #[test]
    fn access_class_not_location_avoid() {
        let s = HotSketch::new();
        assert!(s.broadcast_avoid(2, 0));
        assert!(
            !s.essential_antidep(2, false, false, false),
            "location-wide Avoid is not PredictedEssential"
        );
        s.mark_access_class(2, 6);
        assert!(s.access_class_predicted(2, 6));
        assert!(!s.access_class_predicted(2, 12));
        assert!(s.essential_antidep(2, false, false, false));
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
    fn quiet_fanout_only_does_not_seed_h() {
        let s = HotSketch::new();
        s.seed_from_prior_morph(
            &[TopLocPrior {
                location: 11,
                fanout_ema: 64.0,
                abort_rate: 0.05,
                chain_len_ema: 4.0,
                k_template: 6,
            }],
            false,
            true,
        );
        assert!(
            !s.in_h(11),
            "quiet + fanout-only must not plant a Fence prior"
        );
        s.seed_from_prior_morph(
            &[TopLocPrior {
                location: 12,
                fanout_ema: 64.0,
                abort_rate: 0.90,
                chain_len_ema: 4.0,
                k_template: 6,
            }],
            false,
            true,
        );
        assert!(!s.in_h(12), "quiet follow-on must not plant high-abort H");
        assert!(
            !s.access_class_predicted(12, 6),
            "quiet must not seed PredictedEssential"
        );
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
                k_template: 6,
            }],
            true,
            true,
        );
        // quiet + low abort_rate never plants H (fanout-only is a quiet→fan_out leak).
        assert!(!s.in_h(4), "U6: quiet+flip must not seed a Fence prior");
        s.seed_from_prior(
            &[TopLocPrior {
                location: 8,
                fanout_ema: 16.0,
                abort_rate: 0.05,
                chain_len_ema: 3.0,
                k_template: 6,
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
                k_template: 6,
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

    #[test]
    fn done_without_data_installs_storage_residual() {
        let s = HotSketch::new();
        let r = s.install_done_residual(3, 4);
        assert_eq!(r, ResidualBind::Storage { writer: 4 });
        assert_eq!(
            s.residual_bind(3),
            Some(ResidualBind::Storage { writer: 4 })
        );
        s.install_data_residual(3, 2, 1);
        assert!(matches!(
            s.residual_bind(3),
            Some(ResidualBind::Data {
                tx_idx: 2,
                tx_incarnation: 1
            })
        ));
        // Done after Data keeps the Data residual.
        assert!(matches!(
            s.install_done_residual(3, 9),
            ResidualBind::Data { tx_idx: 2, .. }
        ));
    }

    #[test]
    fn ready_spine_writers_covers_secondary_l() {
        let s = HotSketch::new();
        s.broadcast_avoid(1, 2);
        s.mark_access_class(1, 6);
        s.push_spine(1, 5);
        s.broadcast_avoid(9, 3);
        s.mark_access_class(9, 6);
        s.push_spine(9, 7);
        let ready = s.ready_spine_writers(10, |w| w == 5 || w == 7, |w| w == 2 || w == 3);
        assert_eq!(
            ready,
            vec![5, 7],
            "secondary ℓ Ready writers are PreferAdmit"
        );
    }
}
