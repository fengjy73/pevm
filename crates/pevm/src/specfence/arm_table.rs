//! ArmTable + intra-block learn + inter-block prior snapshot (SF-PS L-intra / L-inter).
//!
//! Learn's only legal outputs: G / ArmTable / release / explore quota.
//! Never a shell flag. Thin `w_max`: n≤176 ⇒ w≤2. Under-covered forbids Full.
//! Hot sticky reuse: explore_n = 0.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::learner::InterBlockPrior;
use super::policy::{CostPolicy, LocStrategy, THIN_SHELL_N};
use super::ready_edge::ReadyEdgeTable;
use super::runnable_set::{QueueKind, RunnableSet};
use super::ResolvePlan;

/// Soft=0 arm stored per ℓ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArmKind {
    Opt,
    Defer,
    Win { w: u8 },
    Seg { s: u8 },
    Full,
}

impl ArmKind {
    #[inline]
    pub(crate) fn from_loc(s: LocStrategy) -> Self {
        match s {
            LocStrategy::OptimisticRead => Self::Opt,
            LocStrategy::DeferPlant => Self::Defer,
            LocStrategy::OrderedWindow { w } => Self::Win { w },
            LocStrategy::Segmented { seg_len } => Self::Seg { s: seg_len },
            LocStrategy::FullChain => Self::Full,
        }
    }

    #[inline]
    pub(crate) fn window(self) -> u8 {
        match self {
            Self::Win { w } => w,
            Self::Seg { s } => s,
            Self::Full => u8::MAX,
            _ => 0,
        }
    }

    #[inline]
    pub(crate) fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    #[inline]
    pub(crate) fn to_loc(self) -> LocStrategy {
        match self {
            Self::Opt => LocStrategy::OptimisticRead,
            Self::Defer => LocStrategy::DeferPlant,
            Self::Win { w } => LocStrategy::win(w as usize),
            Self::Seg { s } => LocStrategy::seg(s as usize),
            Self::Full => LocStrategy::FullChain,
        }
    }
}

/// Packed inter-block arm (prior).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArmSnap {
    pub location: MemoryLocationHash,
    pub arm: ArmKind,
    pub sticky: bool,
    pub under_covered: bool,
}

#[derive(Debug)]
struct ArmEntry {
    arm: ArmKind,
    sticky: bool,
    n_pull: AtomicUsize,
    n_reward: AtomicUsize,
    ema_wall_ns: AtomicU64,
    ema_abort_cf_ns: AtomicU64,
    last_update_tick: AtomicU64,
    explore_budget: AtomicUsize,
    under_covered: bool,
    lazy: bool,
}

/// Pending mid-block graph patch (applied at pick boundary only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct IntraPatch {
    pub location: MemoryLocationHash,
    pub new_arm: ArmKind,
}

/// First-class per-ℓ arm table for one Pevm (lives across blocks).
#[derive(Debug)]
pub(crate) struct ArmTable {
    entries: DashMap<MemoryLocationHash, ArmEntry, BuildIdentityHasher>,
    promoted_this_block: DashSet<MemoryLocationHash, BuildIdentityHasher>,
    pending: Mutex<Vec<IntraPatch>>,
    tick: AtomicU64,
    explore_n: AtomicUsize,
    mid_promote_n: AtomicUsize,
    mid_promote_veto_n: AtomicUsize,
    e1_n: AtomicUsize,
    e2_n: AtomicUsize,
    e3_n: AtomicUsize,
    e4_n: AtomicUsize,
    e5_n: AtomicUsize,
    e6_n: AtomicUsize,
    begin_from_prior: AtomicUsize,
    prior_plant_n: AtomicUsize,
}

impl Default for ArmTable {
    fn default() -> Self {
        Self {
            entries: DashMap::default(),
            promoted_this_block: DashSet::default(),
            pending: Mutex::new(Vec::new()),
            tick: AtomicU64::new(0),
            explore_n: AtomicUsize::new(0),
            mid_promote_n: AtomicUsize::new(0),
            mid_promote_veto_n: AtomicUsize::new(0),
            e1_n: AtomicUsize::new(0),
            e2_n: AtomicUsize::new(0),
            e3_n: AtomicUsize::new(0),
            e4_n: AtomicUsize::new(0),
            e5_n: AtomicUsize::new(0),
            e6_n: AtomicUsize::new(0),
            begin_from_prior: AtomicUsize::new(0),
            prior_plant_n: AtomicUsize::new(0),
        }
    }
}

impl ArmTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// PC-4: thin n≤176 ⇒ w≤2. Never Full on lazy / under-covered.
    #[inline]
    pub(crate) fn w_max(block_n: usize, chain_len: usize, _morph_waw: bool) -> u8 {
        if block_n == 0 {
            return 1;
        }
        if block_n <= THIN_SHELL_N {
            return 2;
        }
        if chain_len >= 64 {
            return 2;
        }
        4
    }

    #[inline]
    fn bump_tick(&self) -> u64 {
        self.tick.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Begin: install prior snapshot. Reuse sticky ⇒ explore_budget=0.
    pub(crate) fn begin_from_prior(&self, prior: &InterBlockPrior, reuse: bool) {
        self.promoted_this_block.clear();
        self.pending.lock().unwrap().clear();
        self.explore_n.store(0, Ordering::Relaxed);
        self.mid_promote_n.store(0, Ordering::Relaxed);
        self.mid_promote_veto_n.store(0, Ordering::Relaxed);
        self.e1_n.store(0, Ordering::Relaxed);
        self.e2_n.store(0, Ordering::Relaxed);
        self.e3_n.store(0, Ordering::Relaxed);
        self.e4_n.store(0, Ordering::Relaxed);
        self.e5_n.store(0, Ordering::Relaxed);
        self.e6_n.store(0, Ordering::Relaxed);
        self.prior_plant_n.store(0, Ordering::Relaxed);
        let snaps = prior.arm_snapshot();
        self.begin_from_prior
            .store(if snaps.is_empty() { 0 } else { 1 }, Ordering::Relaxed);
        for snap in snaps {
            let explore = if reuse && snap.sticky { 0 } else { 1 };
            self.entries
                .entry(snap.location)
                .and_modify(|e| {
                    e.arm = snap.arm;
                    e.sticky = snap.sticky;
                    e.under_covered = snap.under_covered;
                    e.explore_budget.store(explore, Ordering::Relaxed);
                })
                .or_insert_with(|| ArmEntry {
                    arm: snap.arm,
                    sticky: snap.sticky,
                    n_pull: AtomicUsize::new(0),
                    n_reward: AtomicUsize::new(0),
                    ema_wall_ns: AtomicU64::new(0),
                    ema_abort_cf_ns: AtomicU64::new(0),
                    last_update_tick: AtomicU64::new(0),
                    explore_budget: AtomicUsize::new(explore),
                    under_covered: snap.under_covered,
                    lazy: false,
                });
        }
        if reuse {
            for e in self.entries.iter_mut() {
                if e.sticky {
                    e.explore_budget.store(0, Ordering::Relaxed);
                }
            }
        }
    }

    /// B4: plant Prior arms into CostPolicy before `admit_seed` so wave-1
    /// `hops_to_admit` / Detect.G match ArmTable (not a cold re-select).
    /// Lazy / under-covered Full stay Opt. Thin caps `w`.
    pub(crate) fn install_prior_into_policy(&self, policy: &CostPolicy, block_n: usize) -> usize {
        let w_cap = Self::w_max(block_n, 0, false);
        let mut planted = 0;
        for e in self.entries.iter() {
            let loc = *e.key();
            if e.lazy || policy.loc_forbids_ordered(loc) {
                policy.remember_arm(loc, LocStrategy::OptimisticRead);
                continue;
            }
            if e.under_covered && e.arm.is_full() {
                policy.remember_arm(loc, LocStrategy::OptimisticRead);
                continue;
            }
            let mut arm = e.arm.to_loc();
            if let LocStrategy::OrderedWindow { w } = arm {
                arm = LocStrategy::win((w as usize).min(w_cap as usize));
            }
            if arm == LocStrategy::FullChain && block_n <= THIN_SHELL_N {
                arm = LocStrategy::win(w_cap as usize);
            }
            policy.remember_arm(loc, arm);
            if arm.is_ordered() {
                policy.promote_short_edge(loc, 0);
                planted += 1;
            }
        }
        self.prior_plant_n.store(planted, Ordering::Relaxed);
        planted
    }

    /// End: pack into inter-block prior. Thin / under-covered force Opt.
    pub(crate) fn end_pack(&self, prior: &InterBlockPrior, block_n: usize) {
        let mut snaps = Vec::new();
        for e in self.entries.iter() {
            let mut arm = e.arm;
            if block_n <= THIN_SHELL_N && arm.window() > 2 {
                arm = ArmKind::Win { w: 2 };
            }
            if e.under_covered && arm.is_full() {
                arm = ArmKind::Opt;
            }
            if e.lazy {
                arm = ArmKind::Opt;
            }
            snaps.push(ArmSnap {
                location: *e.key(),
                arm,
                sticky: e.sticky,
                under_covered: e.under_covered,
            });
        }
        prior.pack_arm_snapshot(snaps);
    }

    #[inline]
    pub(crate) fn began_from_prior(&self) -> bool {
        self.begin_from_prior.load(Ordering::Relaxed) != 0
    }

    #[inline]
    pub(crate) fn explore_n(&self) -> usize {
        self.explore_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn mid_promote_n(&self) -> usize {
        self.mid_promote_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn mid_promote_veto_n(&self) -> usize {
        self.mid_promote_veto_n.load(Ordering::Relaxed)
    }

    /// E1–E6 observe. Heavy rebind is queued, not done in the EVM frame.
    pub(crate) fn observe(
        &self,
        plan: ResolvePlan,
        location: Option<MemoryLocationHash>,
        lazy: bool,
        unfenced_storm: bool,
        block_n: usize,
        chain_len: usize,
    ) {
        let tick = self.bump_tick();
        let Some(loc) = location else {
            match plan {
                ResolvePlan::PartialAbortRebind | ResolvePlan::PartialAbortRewind => {
                    self.e3_n.fetch_add(1, Ordering::Relaxed);
                }
                ResolvePlan::FullReplay => {
                    self.e2_n.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
            return;
        };
        if lazy {
            self.e6_n.fetch_add(1, Ordering::Relaxed);
            self.force_opt(loc, true);
            return;
        }
        match plan {
            ResolvePlan::Commit => {
                self.e1_n.fetch_add(1, Ordering::Relaxed);
                self.reward(loc, true, tick);
            }
            ResolvePlan::PartialAbortRebind | ResolvePlan::PartialAbortRewind => {
                self.e3_n.fetch_add(1, Ordering::Relaxed);
                self.reward(loc, true, tick);
            }
            ResolvePlan::OrderedReplay => {
                self.reward(loc, true, tick);
            }
            ResolvePlan::FullReplay => {
                self.e2_n.fetch_add(1, Ordering::Relaxed);
                self.reward(loc, false, tick);
                if unfenced_storm {
                    self.e5_n.fetch_add(1, Ordering::Relaxed);
                    self.mark_under_covered(loc);
                } else {
                    self.maybe_queue_promote(loc, block_n, chain_len);
                }
            }
        }
    }

    fn reward(&self, loc: MemoryLocationHash, positive: bool, tick: u64) {
        self.entries
            .entry(loc)
            .and_modify(|e| {
                e.n_reward.fetch_add(1, Ordering::Relaxed);
                e.last_update_tick.store(tick, Ordering::Relaxed);
                if positive {
                    e.ema_wall_ns.fetch_add(1, Ordering::Relaxed);
                } else {
                    e.ema_abort_cf_ns.fetch_add(1, Ordering::Relaxed);
                }
            })
            .or_insert_with(|| ArmEntry {
                arm: ArmKind::Opt,
                sticky: false,
                n_pull: AtomicUsize::new(0),
                n_reward: AtomicUsize::new(1),
                ema_wall_ns: AtomicU64::new(u64::from(positive)),
                ema_abort_cf_ns: AtomicU64::new(u64::from(!positive)),
                last_update_tick: AtomicU64::new(tick),
                explore_budget: AtomicUsize::new(0),
                under_covered: false,
                lazy: false,
            });
    }

    fn force_opt(&self, loc: MemoryLocationHash, lazy: bool) {
        self.entries
            .entry(loc)
            .and_modify(|e| {
                e.arm = ArmKind::Opt;
                e.lazy = lazy;
                e.sticky = true;
                e.explore_budget.store(0, Ordering::Relaxed);
            })
            .or_insert_with(|| ArmEntry {
                arm: ArmKind::Opt,
                sticky: true,
                n_pull: AtomicUsize::new(0),
                n_reward: AtomicUsize::new(0),
                ema_wall_ns: AtomicU64::new(0),
                ema_abort_cf_ns: AtomicU64::new(0),
                last_update_tick: AtomicU64::new(0),
                explore_budget: AtomicUsize::new(0),
                under_covered: false,
                lazy,
            });
    }

    fn mark_under_covered(&self, loc: MemoryLocationHash) {
        self.entries
            .entry(loc)
            .and_modify(|e| {
                e.under_covered = true;
                e.arm = ArmKind::Opt;
                e.sticky = true;
                e.explore_budget.store(0, Ordering::Relaxed);
            })
            .or_insert_with(|| ArmEntry {
                arm: ArmKind::Opt,
                sticky: true,
                n_pull: AtomicUsize::new(0),
                n_reward: AtomicUsize::new(0),
                ema_wall_ns: AtomicU64::new(0),
                ema_abort_cf_ns: AtomicU64::new(0),
                last_update_tick: AtomicU64::new(0),
                explore_budget: AtomicUsize::new(0),
                under_covered: true,
                lazy: false,
            });
    }

    fn maybe_queue_promote(&self, loc: MemoryLocationHash, block_n: usize, chain_len: usize) {
        if self.promoted_this_block.contains(&loc) {
            return;
        }
        if self.entries.get(&loc).is_some_and(|e| e.lazy || e.under_covered) {
            return;
        }
        let cap = Self::w_max(block_n, chain_len, false);
        let cur = self.entries.get(&loc).map(|e| e.arm.window()).unwrap_or(0);
        let next = (cur.saturating_add(1)).min(cap).max(1);
        self.pending.lock().unwrap().push(IntraPatch {
            location: loc,
            new_arm: ArmKind::Win { w: next },
        });
    }

    /// Pick-boundary IntraPatch. PC may veto a promote that would drain |R|<C.
    pub(crate) fn apply_pending_patches(
        &self,
        runnable: &RunnableSet,
        ready: &ReadyEdgeTable,
        policy: Option<&CostPolicy>,
        cores: usize,
        block_n: usize,
    ) -> usize {
        let patches = {
            let mut g = self.pending.lock().unwrap();
            std::mem::take(&mut *g)
        };
        let mut applied = 0;
        for p in patches {
            if p.new_arm.window() as usize > Self::w_max(block_n, 0, false) as usize {
                self.mid_promote_veto_n.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.promoted_this_block.contains(&p.location) {
                continue;
            }
            if policy.is_some_and(|pol| {
                pol.loc_forbids_ordered(p.location) || pol.is_optimistic_majority_block()
            }) {
                self.mid_promote_veto_n.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let consumers: Vec<TxIdx> = ready
                .consumers_queued_on(p.location)
                .into_iter()
                .filter(|&c| !ready.is_started(c))
                .collect();
            let width = runnable.width();
            if width.saturating_sub(consumers.len()) < cores && !consumers.is_empty() {
                self.mid_promote_veto_n.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            for c in consumers {
                runnable.remove_from_indep(c);
                if ready.may_execute(c) {
                    runnable.push(c, QueueKind::Released);
                } else {
                    runnable.mark_wait(c);
                }
            }
            self.promoted_this_block.insert(p.location);
            self.entries
                .entry(p.location)
                .and_modify(|e| {
                    e.arm = p.new_arm;
                    e.sticky = false;
                })
                .or_insert_with(|| ArmEntry {
                    arm: p.new_arm,
                    sticky: false,
                    n_pull: AtomicUsize::new(0),
                    n_reward: AtomicUsize::new(0),
                    ema_wall_ns: AtomicU64::new(0),
                    ema_abort_cf_ns: AtomicU64::new(0),
                    last_update_tick: AtomicU64::new(0),
                    explore_budget: AtomicUsize::new(0),
                    under_covered: false,
                    lazy: false,
                });
            if let Some(pol) = policy {
                pol.promote_short_edge(p.location, 0);
            }
            self.mid_promote_n.fetch_add(1, Ordering::Relaxed);
            applied += 1;
        }
        applied
    }

    #[inline]
    pub(crate) fn note_explore(&self) {
        self.explore_n.fetch_add(1, Ordering::Relaxed);
    }

    /// E4: refuse_fill that immediately ran an independent (PC positive).
    #[inline]
    pub(crate) fn note_e4(&self) {
        self.e4_n.fetch_add(1, Ordering::Relaxed);
    }

    /// E6 graph change: drop leftover OrderedAdmit on lazy ℓ and return
    /// not-yet-started consumers to Q_indep. Observe already force_opt.
    pub(crate) fn demote_lazy_graph(
        &self,
        loc: MemoryLocationHash,
        ready: &ReadyEdgeTable,
        runnable: &RunnableSet,
    ) {
        for c in ready.consumers_queued_on(loc) {
            if ready.is_started(c) {
                continue;
            }
            ready.ungate(c);
            runnable.force_push(c, QueueKind::Indep);
        }
    }

    #[inline]
    pub(crate) fn e1_n(&self) -> usize {
        self.e1_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn e2_n(&self) -> usize {
        self.e2_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn e3_n(&self) -> usize {
        self.e3_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn e4_n(&self) -> usize {
        self.e4_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn e5_n(&self) -> usize {
        self.e5_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn e6_n(&self) -> usize {
        self.e6_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn prior_plant_n(&self) -> usize {
        self.prior_plant_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn arm_of(&self, loc: MemoryLocationHash) -> Option<ArmKind> {
        self.entries.get(&loc).map(|e| e.arm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thin_w_max_is_two() {
        assert_eq!(ArmTable::w_max(176, 20, false), 2);
        assert_eq!(ArmTable::w_max(64, 8, false), 2);
        assert!(ArmTable::w_max(512, 8, false) <= 4);
    }

    #[test]
    fn reuse_sticky_explore_budget_zero() {
        let prior = InterBlockPrior::new();
        prior.pack_arm_snapshot(vec![ArmSnap {
            location: 11,
            arm: ArmKind::Win { w: 2 },
            sticky: true,
            under_covered: false,
        }]);
        let t = ArmTable::new();
        t.begin_from_prior(&prior, true);
        assert!(t.began_from_prior());
        assert_eq!(t.explore_n(), 0);
        let e = t.entries.get(&11).expect("prior arm");
        assert_eq!(e.explore_budget.load(Ordering::Relaxed), 0);
        assert!(e.sticky);
    }

    #[test]
    fn under_covered_end_pack_forbids_full() {
        let t = ArmTable::new();
        t.mark_under_covered(3);
        t.entries.get_mut(&3).unwrap().arm = ArmKind::Full;
        let prior = InterBlockPrior::new();
        t.end_pack(&prior, 200);
        let snap = prior.arm_snapshot();
        assert!(snap.iter().any(|s| s.location == 3 && matches!(s.arm, ArmKind::Opt)));
    }

    #[test]
    fn install_prior_seeds_policy_block_arm() {
        let prior = InterBlockPrior::new();
        prior.pack_arm_snapshot(vec![ArmSnap {
            location: 0x32be,
            arm: ArmKind::Win { w: 2 },
            sticky: true,
            under_covered: false,
        }]);
        let t = ArmTable::new();
        t.begin_from_prior(&prior, true);
        let p = CostPolicy::new();
        p.begin_block(300);
        p.note_short_pair(0x32be, 4, 31);
        p.promote_short_edge(0x32be, 0);
        let n = t.install_prior_into_policy(&p, 300);
        assert!(n >= 1, "prior Win must plant into CostPolicy");
        assert_eq!(p.loc_strategy(0x32be, 1), LocStrategy::win(2));
        assert!(
            p.hops_to_admit(0x32be, 1) > 0,
            "B4: Prior arm must make wave-1 hops_to_admit > 0"
        );
        assert_eq!(t.explore_n(), 0);
    }

    #[test]
    fn intra_patch_moves_only_this_location() {
        let ready = ReadyEdgeTable::new();
        ready.note_consumer_on(3, 0, Some(11));
        ready.note_consumer_on(5, 1, Some(22));
        let r = RunnableSet::new(8, 2);
        r.push(3, QueueKind::Indep);
        r.push(5, QueueKind::Indep);
        r.push(6, QueueKind::Indep);
        let t = ArmTable::new();
        t.pending.lock().unwrap().push(IntraPatch {
            location: 11,
            new_arm: ArmKind::Win { w: 1 },
        });
        let applied = t.apply_pending_patches(&r, &ready, None, 2, 8);
        assert_eq!(applied, 1);
        assert_eq!(t.mid_promote_n(), 1);
        // 5 is on a different ℓ — must stay independent, not WAIT.
        let first = r.pick(2, &ready);
        match first {
            Some(super::super::runnable_set::SfPick::Execute { tx, .. }) => {
                assert_ne!(tx, 3, "promoted consumer of ℓ=11 left Q_indep");
            }
            other => panic!("expected an independent execute, got {other:?}"),
        }
    }

    #[test]
    fn pc_veto_when_width_would_collapse() {
        let ready = ReadyEdgeTable::new();
        ready.note_consumer_on(1, 0, Some(11));
        ready.note_consumer_on(2, 0, Some(11));
        let r = RunnableSet::new(4, 4);
        r.push(1, QueueKind::Indep);
        r.push(2, QueueKind::Indep);
        let t = ArmTable::new();
        t.pending.lock().unwrap().push(IntraPatch {
            location: 11,
            new_arm: ArmKind::Win { w: 1 },
        });
        let applied = t.apply_pending_patches(&r, &ready, None, 4, 4);
        assert_eq!(applied, 0, "PC veto: width 2 − 2 < 4 cores");
        assert_eq!(t.mid_promote_veto_n(), 1);
        assert_eq!(t.mid_promote_n(), 0);
    }
}
