//! Per-access Avoid arm: Opt | WaitOnce | NeverWait.
//!
//! Shared across workers (with MvMemory, the scheduler's live writer, and
//! the arm table). One wait per `(tx, ℓ, w)` until that writer publishes or
//! is done. A second Blocking of the same triple is refused. Beneficiary and
//! basic-lazy are NeverWait. Sticky Opt is not an arm.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::{MemoryLocationHash, TxIdx};

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
    /// `u64::MAX` = no learned chain. Writers are for demand-driven WaitOnce.
    crit_loc: AtomicU64,
    crit_writers: Mutex<Vec<TxIdx>>,
}

impl AccessArmTable {
    pub(crate) fn new() -> Self {
        let t = Self::default();
        t.crit_loc.store(u64::MAX, Ordering::Relaxed);
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
        self.crit_loc.store(u64::MAX, Ordering::Relaxed);
        self.crit_writers.lock().unwrap().clear();
        for (loc, tag, k) in prior.access_arm_snapshot() {
            let arm = AccessArm::from_tag(tag);
            if arm == AccessArm::Opt {
                continue;
            }
            self.arms.insert(loc, ArmRec { arm, k, hits: 0, peer: 0 });
        }
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
            if flipped && arm == AccessArm::WaitOnce && e.hits == 0 && e.k == 0 {
                arm = AccessArm::Opt;
            }
            if arm == AccessArm::Opt {
                continue;
            }
            snaps.push((*e.key(), arm.tag(), e.k));
        }
        prior.pack_access_arms(snaps);
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

    /// Reuse: shared basic + ascending writers. Nearest pred is the publish
    /// the next writer must see. Hold is planted separately for long spines.
    pub(crate) fn install_crit_chain(&self, loc: MemoryLocationHash, writers: &[TxIdx]) {
        self.crit_loc.store(loc, Ordering::Relaxed);
        *self.crit_writers.lock().unwrap() = writers.to_vec();
        self.note_early_waw(loc, 1);
    }

    #[inline]
    pub(crate) fn crit_loc_hash(&self) -> MemoryLocationHash {
        self.crit_loc.load(Ordering::Relaxed)
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

    /// Avoid-up-front pred for a WaitOnce ℓ: crit chain, else last noted peer.
    pub(crate) fn wait_once_pred(&self, tx: TxIdx, loc: MemoryLocationHash) -> Option<TxIdx> {
        if let Some(p) = self.crit_pred(tx, loc) {
            return Some(p);
        }
        self.arms.get(&loc).and_then(|e| {
            let p = e.peer;
            (p > 0 && p < tx).then_some(p)
        })
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
        prior.pack_access_arms(vec![(11, 1, 0), (33, 1, 5), (22, 2, 1)]);
        prior.force_flipped_for_test();
        let t = AccessArmTable::new();
        t.begin_from_prior(&prior);
        t.end_pack(&prior);
        let snaps = prior.access_arm_snapshot();
        assert!(
            snaps.iter().all(|&(loc, _, _)| loc != 11),
            "unreinforced k=0 WaitOnce decays on morph flip"
        );
        assert!(
            snaps.iter().any(|&(loc, tag, k)| loc == 33 && tag == 1 && k == 5),
            "early-WAW WaitOnce (k>0) survives morph flip"
        );
        assert!(
            snaps.iter().any(|&(loc, tag, _)| loc == 22 && tag == 2),
            "NeverWait prior stays"
        );
    }
}
