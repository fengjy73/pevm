//! Per-access Avoid arm: Opt | WaitOnce | NeverWait.
//!
//! Shared across workers (with MvMemory, the scheduler's live writer, and
//! the arm table). One wait per `(tx, ℓ, w)` until that writer publishes or
//! is done. A second Blocking of the same triple is refused. Beneficiary and
//! basic-lazy are NeverWait. Sticky Opt is not an arm.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::learner::InterBlockPrior;

/// Avoid arm for one location. Opt is the absence of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessArm {
    Opt,
    WaitOnce,
    NeverWait,
}

impl AccessArm {
    fn from_tag(tag: u8) -> Self {
        match tag {
            2 => Self::NeverWait,
            1 => Self::WaitOnce,
            _ => Self::Opt,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::Opt => 0,
            Self::WaitOnce => 1,
            Self::NeverWait => 2,
        }
    }
}

/// What the read does when the multi-version tip is an unfinished writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveAct {
    /// Park once. Publish or done releases the waiter.
    Block,
    /// Do not park. Read the last published value or storage.
    Skip,
    /// Writer already published or finished. Re-read now.
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WaitKey {
    tx: u32,
    writer: u32,
    loc: u64,
}

#[derive(Debug, Clone, Copy)]
struct ArmRec {
    arm: AccessArm,
    k: u32,
    /// Reinforced this block (early WAW or an actual wait).
    hits: u32,
    /// Last observed unfinished pred for this ℓ (Avoid-up-front).
    peer: TxIdx,
}

/// Shared waiter + per-location arm. Not a per-worker copy.
#[derive(Debug, Default)]
pub(crate) struct AccessArmTable {
    arms: DashMap<MemoryLocationHash, ArmRec>,
    waited: DashMap<WaitKey, u8>,
    wait_once: AtomicUsize,
    wait_suppressed: AtomicUsize,
    never_wait: AtomicUsize,
    prefix_resume: AtomicUsize,
    prefix_keep_n: AtomicUsize,
    /// Fast Soft=0 Opt skip: any WaitOnce arm installed this block.
    any_wait_once: std::sync::atomic::AtomicBool,
    /// WaitOnce on a location that is not the installed crit chain.
    /// While this is false, a non-chain first Opt can skip the arm DashMap.
    non_crit_wait: std::sync::atomic::AtomicBool,
    /// Locations protected this block after the first hot conflict.
    /// Later reads must WaitOnce before Opt-discovering ℓ again.
    protected: DashSet<MemoryLocationHash, BuildIdentityHasher>,
    protect_n: AtomicUsize,
    protect_before_opt_n: AtomicUsize,
    replay_after_protect_n: AtomicUsize,
    protect_live: std::sync::atomic::AtomicBool,
    /// `u64::MAX` = no learned chain. Writers are for demand-driven WaitOnce.
    crit_loc: AtomicU64,
    crit_writers: Mutex<Vec<TxIdx>>,
    /// Prior-block touchers, not armed until the first hot conflict on `ℓ`.
    prior_loc: AtomicU64,
    prior_writers: Mutex<Vec<TxIdx>>,
    /// Detect (a) edges for next pick / next block: (consumer, producer, ℓ).
    wait_edges: Mutex<Vec<(TxIdx, TxIdx, MemoryLocationHash)>>,
}

impl AccessArmTable {
    pub(crate) fn new() -> Self {
        let t = Self::default();
        t.crit_loc.store(u64::MAX, Ordering::Relaxed);
        t.prior_loc.store(u64::MAX, Ordering::Relaxed);
        t
    }

    /// Install the previous block's arms. A morph flip already decayed
    /// unreinforced WaitOnce at the previous `end_pack`.
    pub(crate) fn begin_from_prior(&self, prior: &InterBlockPrior) {
        self.arms.clear();
        self.waited.clear();
        self.wait_once.store(0, Ordering::Relaxed);
        self.wait_suppressed.store(0, Ordering::Relaxed);
        self.never_wait.store(0, Ordering::Relaxed);
        self.prefix_resume.store(0, Ordering::Relaxed);
        self.prefix_keep_n.store(0, Ordering::Relaxed);
        self.any_wait_once.store(false, Ordering::Relaxed);
        self.non_crit_wait.store(false, Ordering::Relaxed);
        self.protected.clear();
        self.protect_n.store(0, Ordering::Relaxed);
        self.protect_before_opt_n.store(0, Ordering::Relaxed);
        self.replay_after_protect_n.store(0, Ordering::Relaxed);
        self.protect_live.store(false, Ordering::Relaxed);
        self.crit_loc.store(u64::MAX, Ordering::Relaxed);
        self.crit_writers.lock().unwrap().clear();
        self.prior_loc.store(u64::MAX, Ordering::Relaxed);
        self.prior_writers.lock().unwrap().clear();
        self.wait_edges.lock().unwrap().clear();
        for (loc, tag, k, peer) in prior.access_arm_snapshot() {
            let arm = AccessArm::from_tag(tag);
            if arm == AccessArm::Opt {
                continue;
            }
            if arm == AccessArm::WaitOnce {
                self.any_wait_once.store(true, Ordering::Relaxed);
            }
            self.arms.insert(
                loc,
                ArmRec {
                    arm,
                    k,
                    hits: 0,
                    peer,
                },
            );
        }
        *self.wait_edges.lock().unwrap() = prior.access_wait_edge_snapshot();
        // Detect(a): edges refresh peer so WaitOnce pred is known before read.
        for &(_c, p, loc) in self.wait_edges.lock().unwrap().iter() {
            if p == 0 {
                continue;
            }
            if let Some(mut e) = self.arms.get_mut(&loc) {
                if e.arm == AccessArm::WaitOnce && p > e.peer {
                    e.peer = p;
                }
            }
        }
        self.refresh_non_crit_wait();
    }

    /// Pack prior. Morph flip decays WaitOnce that this block did not
    /// reinforce. NeverWait (beneficiary / basic-lazy) stays.
    pub(crate) fn end_pack(&self, prior: &InterBlockPrior) {
        let flipped = prior.last_flipped_peek();
        let mut snaps = Vec::new();
        for e in self.arms.iter() {
            let mut arm = e.arm;
            // Morph flip decays opportunistic WaitOnce (k==0, never reinforced).
            // Early-WAW templates (k>0) stay — reuse must not re-Opt the same fail_k.
            // Mid-block hot protect sticks for the rest of this block and the
            // next prior. Learn must not demote it back to Opt.
            if self.protected.contains(e.key()) && arm == AccessArm::WaitOnce {
                // keep
            } else if flipped && arm == AccessArm::WaitOnce && e.hits == 0 && e.k == 0 {
                arm = AccessArm::Opt;
            }
            if arm == AccessArm::Opt {
                continue;
            }
            snaps.push((*e.key(), arm.tag(), e.k, e.peer));
        }
        prior.pack_access_arms(snaps);
        let edges = self.wait_edges.lock().unwrap().clone();
        prior.pack_access_wait_edges(edges);
    }

    /// Record a WaitOnce consumer→producer edge for Detect (a) plant.
    pub(crate) fn note_wait_edge(&self, consumer: TxIdx, producer: TxIdx, loc: MemoryLocationHash) {
        if producer == 0 || producer >= consumer {
            return;
        }
        let mut edges = self.wait_edges.lock().unwrap();
        if !edges
            .iter()
            .any(|&(c, p, l)| c == consumer && p == producer && l == loc)
        {
            edges.push((consumer, producer, loc));
        }
    }

    /// Plant restored WaitOnce edges without mark_gated (thin Avoid).
    pub(crate) fn plant_wait_edges(&self, ready: &super::ReadyEdgeTable) -> usize {
        let edges = self.wait_edges.lock().unwrap().clone();
        let mut n = 0;
        for (consumer, producer, _loc) in edges {
            ready.note_ungated_wait_on(consumer, producer);
            n += 1;
        }
        n
    }

    #[inline]
    pub(crate) fn is_never(&self, loc: MemoryLocationHash) -> bool {
        self.arms
            .get(&loc)
            .is_some_and(|e| e.arm == AccessArm::NeverWait)
    }

    #[inline]
    pub(crate) fn is_wait_once(&self, loc: MemoryLocationHash) -> bool {
        self.arms
            .get(&loc)
            .is_some_and(|e| e.arm == AccessArm::WaitOnce)
    }

    /// Soft=0 Opt fast path: no WaitOnce arm in this block.
    #[inline]
    pub(crate) fn any_wait_once(&self) -> bool {
        self.any_wait_once.load(Ordering::Relaxed)
    }

    /// True when some WaitOnce location is not the crit chain location.
    #[inline]
    pub(crate) fn non_crit_wait(&self) -> bool {
        self.non_crit_wait.load(Ordering::Relaxed)
    }

    fn mark_non_crit_wait(&self, loc: MemoryLocationHash) {
        let crit = self.crit_loc.load(Ordering::Relaxed);
        if crit == u64::MAX || crit != loc {
            self.non_crit_wait.store(true, Ordering::Relaxed);
        }
    }

    /// Recompute after crit install. A prior WaitOnce on the crit loc alone
    /// must not force every cold read through the arm map.
    pub(crate) fn refresh_non_crit_wait(&self) {
        let crit = self.crit_loc.load(Ordering::Relaxed);
        let extra = self
            .arms
            .iter()
            .any(|e| e.arm == AccessArm::WaitOnce && (crit == u64::MAX || *e.key() != crit));
        self.non_crit_wait.store(extra, Ordering::Relaxed);
    }

    /// Remember who touched `ℓ` last block. WaitOnce edges are installed
    /// only when this block's first hot conflict protects `ℓ`.
    pub(crate) fn note_prior_touchers(&self, loc: MemoryLocationHash, writers: &[TxIdx]) {
        self.prior_loc.store(loc, Ordering::Relaxed);
        *self.prior_writers.lock().unwrap() = writers.to_vec();
    }

    fn install_protect_edges(&self, loc: MemoryLocationHash) {
        let writers = if self.crit_loc.load(Ordering::Relaxed) == loc {
            self.crit_writers.lock().unwrap().clone()
        } else if self.prior_loc.load(Ordering::Relaxed) == loc {
            self.prior_writers.lock().unwrap().clone()
        } else {
            return;
        };
        for w in writers.windows(2) {
            self.note_wait_edge(w[1], w[0], loc);
        }
    }

    /// Reuse: shared basic + ascending writers. Nearest pred is the publish
    /// the next writer must see. Hold is planted separately for long spines.
    pub(crate) fn install_crit_chain(&self, loc: MemoryLocationHash, writers: &[TxIdx]) {
        self.crit_loc.store(loc, Ordering::Relaxed);
        *self.crit_writers.lock().unwrap() = writers.to_vec();
        self.note_early_waw(loc, 1);
        // note_early_waw may have set the flag before crit was visible to a
        // racing reader; this block is still single-threaded. Recompute so a
        // crit-only arm does not tax cold antichain reads.
        self.refresh_non_crit_wait();
    }

    #[inline]
    pub(crate) fn crit_loc_hash(&self) -> MemoryLocationHash {
        self.crit_loc.load(Ordering::Relaxed)
    }

    /// Learned sticky spine length (0 when no crit chain).
    #[inline]
    pub(crate) fn crit_chain_len(&self) -> usize {
        if self.crit_loc.load(Ordering::Relaxed) == u64::MAX {
            return 0;
        }
        self.crit_writers.lock().unwrap().len()
    }

    /// True when `loc` is the sticky ≥32 crit chain location.
    #[inline]
    pub(crate) fn is_crit_loc(&self, loc: MemoryLocationHash) -> bool {
        let c = self.crit_loc.load(Ordering::Relaxed);
        c != u64::MAX && c == loc
    }

    /// Early-WAW template k for `loc` (0 = opportunistic / RAW WaitOnce).
    #[inline]
    pub(crate) fn wait_once_k(&self, loc: MemoryLocationHash) -> u32 {
        self.arms.get(&loc).map(|e| e.k).unwrap_or(0)
    }

    /// Learned chain/prior location this tx is known to touch, if any.
    pub(crate) fn known_toucher_loc(&self, tx: TxIdx) -> Option<MemoryLocationHash> {
        let crit = self.crit_loc.load(Ordering::Relaxed);
        if crit != u64::MAX && self.crit_writers.lock().unwrap().iter().any(|&w| w == tx) {
            return Some(crit);
        }
        let prior = self.prior_loc.load(Ordering::Relaxed);
        if prior != u64::MAX && self.prior_writers.lock().unwrap().iter().any(|&w| w == tx) {
            return Some(prior);
        }
        None
    }

    /// Sorted known writers of `loc` (crit chain, else prior-block touchers).
    pub(crate) fn known_writer_indexes(&self, loc: MemoryLocationHash) -> Option<Vec<TxIdx>> {
        if self.crit_loc.load(Ordering::Relaxed) == loc {
            return Some(self.crit_writers.lock().unwrap().clone());
        }
        if self.prior_loc.load(Ordering::Relaxed) == loc {
            return Some(self.prior_writers.lock().unwrap().clone());
        }
        None
    }

    /// Immediate prior writer on the learned chain, if `loc` is that chain.
    pub(crate) fn crit_pred(&self, tx: TxIdx, loc: MemoryLocationHash) -> Option<TxIdx> {
        if self.crit_loc.load(Ordering::Relaxed) != loc {
            return None;
        }
        let writers = self.crit_writers.lock().unwrap();
        let i = writers.partition_point(|&w| w < tx);
        if i == 0 { None } else { Some(writers[i - 1]) }
    }

    /// Avoid-up-front pred for a WaitOnce ℓ: crit chain, arm peer, else the
    /// per-consumer edge. Arm peer stays 0 for mid-block protect so unrelated
    /// txs keep the thin OCC-shaped skip; the edge names only that toucher.
    pub(crate) fn wait_once_pred(&self, tx: TxIdx, loc: MemoryLocationHash) -> Option<TxIdx> {
        if let Some(p) = self.crit_pred(tx, loc) {
            return Some(p);
        }
        if let Some(p) = self.arms.get(&loc).and_then(|e| {
            let p = e.peer;
            (p > 0 && p < tx).then_some(p)
        }) {
            return Some(p);
        }
        self.nearest_edge_pred(tx, loc)
    }

    /// Latest producer this consumer was told to wait for on `loc`.
    fn nearest_edge_pred(&self, tx: TxIdx, loc: MemoryLocationHash) -> Option<TxIdx> {
        let edges = self.wait_edges.lock().unwrap();
        let mut best: Option<TxIdx> = None;
        for &(c, p, l) in edges.iter() {
            if c == tx && l == loc && p > 0 && p < tx {
                best = Some(best.map_or(p, |b| b.max(p)));
            }
        }
        best
    }

    /// All WaitOnce (loc, peer) pairs with `peer < tx` — Detect before execute.
    pub(crate) fn wait_once_peers_before(&self, tx: TxIdx) -> Vec<(MemoryLocationHash, TxIdx)> {
        let mut out = Vec::new();
        let crit = self.crit_loc.load(Ordering::Relaxed);
        if crit != u64::MAX
            && let Some(p) = self.crit_pred(tx, crit)
        {
            out.push((crit, p));
        }
        for e in self.arms.iter() {
            if e.arm != AccessArm::WaitOnce {
                continue;
            }
            let loc = *e.key();
            let p = e.peer;
            if p > 0 && p < tx {
                if out.iter().any(|&(l, _)| l == loc) {
                    continue;
                }
                out.push((loc, p));
            }
        }
        out
    }

    /// Cheap: this tx is a WaitOnce consumer of some earlier peer.
    /// Crit-spine membership alone must **not** gate OCC-shaped — thin Soft=0
    /// crit consult is already a near-noop, and including `crit_pred` falsely
    /// taxed ~every tx after the first sticky writer (~0.5 ms calm shell).
    #[inline]
    pub(crate) fn has_wait_once_peer_before(&self, tx: TxIdx) -> bool {
        if !self.any_wait_once() {
            return false;
        }
        if self
            .arms
            .iter()
            .any(|e| e.arm == AccessArm::WaitOnce && e.peer > 0 && e.peer < tx)
        {
            return true;
        }
        self.wait_edges
            .lock()
            .unwrap()
            .iter()
            .any(|&(c, p, _)| c == tx && p > 0 && p < tx)
    }

    /// True when `tx` is a known WaitOnce/crit producer (tip install must stay).
    #[inline]
    pub(crate) fn is_wait_once_producer(&self, tx: TxIdx) -> bool {
        let crit = self.crit_loc.load(Ordering::Relaxed);
        if crit != u64::MAX {
            let writers = self.crit_writers.lock().unwrap();
            if writers.iter().any(|&w| w == tx) {
                return true;
            }
        }
        if !self.any_wait_once() {
            return false;
        }
        self.arms
            .iter()
            .any(|e| e.arm == AccessArm::WaitOnce && e.peer == tx)
            || self
                .wait_edges
                .lock()
                .unwrap()
                .iter()
                .any(|&(_, p, _)| p == tx)
    }

    /// First EffectiveWAW / hot conflict on `ℓ` this block. Remaining reads
    /// of `ℓ` take WaitOnce (true tip), not another Opt→FullReplay.
    /// Peer stays 0 so thin OCC-shaped txs that do not touch `ℓ` keep the
    /// fast path; the protected-loc check is per access.
    pub(crate) fn protect_hot(&self, loc: MemoryLocationHash) -> bool {
        if self.is_never(loc) {
            return false;
        }
        if !self.protected.insert(loc) {
            return false;
        }
        self.note_early_waw(loc, 1);
        self.install_protect_edges(loc);
        self.protect_n.fetch_add(1, Ordering::Relaxed);
        self.protect_live.store(true, Ordering::Relaxed);
        true
    }

    #[inline]
    pub(crate) fn is_protected(&self, loc: MemoryLocationHash) -> bool {
        self.protected.contains(&loc)
    }

    #[inline]
    pub(crate) fn protect_live(&self) -> bool {
        self.protect_live.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn note_protect_before_opt(&self) {
        self.protect_before_opt_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn note_replay_after_protect(&self) {
        self.replay_after_protect_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn protect_n(&self) -> usize {
        self.protect_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn protect_before_opt_n(&self) -> usize {
        self.protect_before_opt_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn replay_after_protect_n(&self) -> usize {
        self.replay_after_protect_n.load(Ordering::Relaxed)
    }

    /// Hot early basic/storage WAW template. The next read of `ℓ` waits
    /// once for a live writer instead of Opt-then-full-replay.
    pub(crate) fn note_early_waw(&self, loc: MemoryLocationHash, k: u32) {
        self.note_early_waw_peer(loc, k, 0);
    }

    /// Same as [`Self::note_early_waw`] with an observed producer peer.
    pub(crate) fn note_early_waw_peer(&self, loc: MemoryLocationHash, k: u32, peer: TxIdx) {
        if k == 0 {
            return;
        }
        self.arms
            .entry(loc)
            .and_modify(|e| {
                if e.arm != AccessArm::NeverWait {
                    e.arm = AccessArm::WaitOnce;
                    e.k = k;
                    e.hits = e.hits.saturating_add(1);
                    if peer > 0 {
                        e.peer = peer;
                    }
                }
            })
            .or_insert(ArmRec {
                arm: AccessArm::WaitOnce,
                k,
                hits: 1,
                peer,
            });
        self.any_wait_once.store(true, Ordering::Relaxed);
        self.mark_non_crit_wait(loc);
    }

    /// WaitOnce + peer for consumer `tx` (Detect before next pick / block).
    pub(crate) fn note_early_waw_edge(
        &self,
        consumer: TxIdx,
        loc: MemoryLocationHash,
        k: u32,
        peer: TxIdx,
    ) {
        self.note_early_waw_peer(loc, k, peer);
        self.note_wait_edge(consumer, peer, loc);
    }

    pub(crate) fn note_prefix_resume(&self, kept: usize) {
        self.prefix_resume.fetch_add(1, Ordering::Relaxed);
        self.prefix_keep_n.fetch_add(kept, Ordering::Relaxed);
    }

    /// Live RAW/WAW read. Beneficiary / lazy / learned NeverWait never park.
    /// The same `(tx, ℓ, w)` parks at most once.
    pub(crate) fn decide(
        &self,
        tx: TxIdx,
        loc: MemoryLocationHash,
        writer: TxIdx,
        never: bool,
        writer_finished: bool,
    ) -> LiveAct {
        if never || self.is_never(loc) {
            self.never_wait.fetch_add(1, Ordering::Relaxed);
            self.arms.insert(
                loc,
                ArmRec {
                    arm: AccessArm::NeverWait,
                    k: 1,
                    hits: 1,
                    peer: 0,
                },
            );
            return LiveAct::Skip;
        }
        if writer_finished {
            return LiveAct::Retry;
        }
        let key = WaitKey {
            tx: tx as u32,
            writer: writer as u32,
            loc,
        };
        match self.waited.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(_) => {
                self.wait_suppressed.fetch_add(1, Ordering::Relaxed);
                LiveAct::Skip
            }
            dashmap::mapref::entry::Entry::Vacant(v) => {
                v.insert(1);
                self.wait_once.fetch_add(1, Ordering::Relaxed);
                let already_never = self
                    .arms
                    .get(&loc)
                    .is_some_and(|e| e.arm == AccessArm::NeverWait);
                if !already_never {
                    self.arms
                        .entry(loc)
                        .and_modify(|e| {
                            e.arm = AccessArm::WaitOnce;
                            e.hits = e.hits.saturating_add(1);
                            if writer > 0 {
                                e.peer = writer;
                            }
                        })
                        .or_insert(ArmRec {
                            arm: AccessArm::WaitOnce,
                            k: 0,
                            hits: 1,
                            peer: writer,
                        });
                }
                self.any_wait_once.store(true, Ordering::Relaxed);
                if !already_never {
                    self.mark_non_crit_wait(loc);
                }
                LiveAct::Block
            }
        }
    }

    #[inline]
    pub(crate) fn wait_once(&self) -> usize {
        self.wait_once.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn wait_suppressed(&self) -> usize {
        self.wait_suppressed.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn never_wait(&self) -> usize {
        self.never_wait.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn prefix_resume(&self) -> usize {
        self.prefix_resume.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beneficiary_never_waits_and_true_waw_waits_once() {
        let t = AccessArmTable::new();
        assert_eq!(t.decide(9, 1, 8, true, false), LiveAct::Skip);
        assert_eq!(t.decide(9, 1, 8, true, false), LiveAct::Skip);
        assert_eq!(t.never_wait(), 2);
        assert_eq!(t.wait_once(), 0);

        assert_eq!(t.decide(329, 7, 314, false, false), LiveAct::Block);
        assert_eq!(
            t.decide(329, 7, 314, false, false),
            LiveAct::Skip,
            "same (tx, ℓ, w) must not Blocking again"
        );
        assert_eq!(t.wait_once(), 1);
        assert_eq!(t.wait_suppressed(), 1);
        assert_eq!(
            t.decide(329, 7, 314, false, true),
            LiveAct::Retry,
            "publish/done releases; the cap is not a second park"
        );
    }

    #[test]
    fn morph_flip_decays_unreinforced_wait_once() {
        let prior = InterBlockPrior::new();
        // k=0 opportunistic WaitOnce decays; k=5 early-WAW template stays.
        // peer=7 on early-WAW so Detect(a) restores pred before read.
        prior.pack_access_arms(vec![(11, 1, 0, 0), (33, 1, 5, 7), (22, 2, 1, 0)]);
        prior.force_flipped_for_test();
        let t = AccessArmTable::new();
        t.begin_from_prior(&prior);
        assert_eq!(t.wait_once_pred(40, 33), Some(7));
        t.end_pack(&prior);
        let snaps = prior.access_arm_snapshot();
        assert!(
            snaps.iter().all(|&(loc, _, _, _)| loc != 11),
            "unreinforced k=0 WaitOnce decays on morph flip"
        );
        assert!(
            snaps
                .iter()
                .any(|&(loc, tag, k, peer)| loc == 33 && tag == 1 && k == 5 && peer == 7),
            "early-WAW WaitOnce (k>0) + peer survives morph flip"
        );
        assert!(
            snaps.iter().any(|&(loc, tag, _, _)| loc == 22 && tag == 2),
            "NeverWait prior stays"
        );
    }

    #[test]
    fn has_wait_once_peer_before_gates_occ_shaped() {
        let t = AccessArmTable::new();
        assert!(!t.has_wait_once_peer_before(40));
        // Crit spine alone must not gate OCC-shaped.
        t.install_crit_chain(99, &[1, 7, 20, 40]);
        assert!(
            !t.non_crit_wait(),
            "crit-only WaitOnce must not tax cold antichain reads"
        );
        assert!(
            !t.has_wait_once_peer_before(40),
            "crit_pred alone must not block OCC-shaped"
        );
        t.note_early_waw_peer(33, 5, 7);
        assert!(t.non_crit_wait(), "non-crit WaitOnce stays on the arm map");
        assert!(t.any_wait_once());
        assert!(t.has_wait_once_peer_before(40));
        assert!(!t.has_wait_once_peer_before(5), "peer must be before tx");
        assert!(t.is_wait_once_producer(7));
        assert!(
            t.is_wait_once_producer(40),
            "crit writer stays a tip producer"
        );
        assert!(!t.is_wait_once_producer(99));
    }

    #[test]
    fn protect_hot_arms_wait_once_without_occ_shaped_peer() {
        let t = AccessArmTable::new();
        t.note_prior_touchers(11, &[3, 7, 40]);
        assert!(t.protect_hot(11));
        assert_eq!(t.wait_once_pred(40, 11), Some(7));
        assert_eq!(t.wait_once_pred(7, 11), Some(3));
        assert!(!t.has_wait_once_peer_before(4));
        assert!(!t.protect_hot(11), "one protect per location");
        assert!(t.is_protected(11));
        assert!(t.is_wait_once(11));
        assert!(t.protect_live());
        assert_eq!(t.protect_n(), 1);
        // Arm peer stays 0. Only the prior touchers gain a WaitOnce edge.
        assert!(!t.has_wait_once_peer_before(2));
        assert!(t.has_wait_once_peer_before(40));
        assert!(!t.has_wait_once_peer_before(6));
        t.note_replay_after_protect();
        t.note_protect_before_opt();
        assert_eq!(t.replay_after_protect_n(), 1);
        assert_eq!(t.protect_before_opt_n(), 1);
    }
}
