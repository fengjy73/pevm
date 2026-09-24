//! AccessEvent spine: three conflict primitives + Advanced Learn.
//!
//! Hot path is Detect on the real read/write, then at most one primitive.
//! Cross-block state is a radar ([`PriorAction::PreheatRadarOnly`]). It never
//! installs an Avoid arm. Soft=0 starts each block with an empty Avoid table.
//!
//! Ideal lower bound, when a caller has both inputs, is
//! `max(L_crit, sum_work / cores)` — never `n / cores`.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use crate::{MemoryLocationHash, TxIdx};

/// Read or write at one interpreter host access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessMode {
    /// World-state read (BALANCE / SLOAD / account load).
    Read,
    /// World-state write effect (committed SSTORE / account write).
    Write,
}

/// Structural conflict class. Chain is a schedule of these, not a fourth primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeClass {
    /// Read must observe the nearest lower writer's true tip.
    Raw,
    /// Later writer publishes only after the prior tip.
    Waw,
    /// Live reader keeps the tip it bound; a later write does not drop it.
    War,
}

/// The only three conflict recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recipe {
    /// RAW: wait for the true published tip, then consume it.
    WaitTrueVersion,
    /// WAW: order the next writer on the true tip (lock is the same wait).
    OrderedTip,
    /// WAR: pin the reader's tip until that reader is done.
    RetainHistory,
}

impl Recipe {
    /// Map a detected class to its primitive. Detect always wins over a sticky recipe.
    pub(crate) const fn for_class(class: EdgeClass) -> Self {
        match class {
            EdgeClass::Raw => Self::WaitTrueVersion,
            EdgeClass::Waw => Self::OrderedTip,
            EdgeClass::War => Self::RetainHistory,
        }
    }
}

/// Block-morph hint carried by the radar. Not an Avoid key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MorphClass {
    #[default]
    /// No hint yet.
    Unknown,
    /// Many readers on one writer set.
    RawFanOut,
    /// RAW and WAW both material.
    MixedRawWaw,
    /// One ordered writer spine.
    WawSpine,
    /// Few edges.
    SparseConflict,
    /// Almost no structural edges.
    NearIndependent,
}

/// What a cross-block prior is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorAction {
    /// Raise radar weights. Never Fence, never OrderedLock.
    PreheatRadarOnly,
}

/// One interpreter host access. `k` is the host-access ordinal; `depth` is the live frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AccessEvent {
    /// Transaction index.
    pub tx: TxIdx,
    /// Structural location hash (basic or storage). Lazy and beneficiary are not events.
    pub loc: MemoryLocationHash,
    /// Access ordinal inside this incarnation.
    pub k: u16,
    /// Call depth. `0` before the first frame (pre-execution loads).
    pub depth: u8,
    /// Read or write.
    pub mode: AccessMode,
}

/// Pointer at a true publish, never an OCC estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionPointer {
    /// Writer that published the tip.
    pub writer: TxIdx,
    /// Incarnation of that publish.
    pub incarnation: u32,
    /// True data, not a claim.
    pub released: bool,
}

/// Same-block record for one structural location. Recipe is the last Detect, not a lock.
#[derive(Debug, Clone)]
struct IntraRecord {
    last_class: EdgeClass,
    /// Sticky default. A later Detect with a different class overrides it.
    recipe: Recipe,
    first_tx: TxIdx,
    first_k: u16,
    peer: Option<TxIdx>,
    writers: Vec<TxIdx>,
    readers: Vec<TxIdx>,
    /// Retained tips (reader, writer) that a later write must not drop.
    retained: Vec<(TxIdx, TxIdx)>,
    ok_ema: f32,
    fail_ema: f32,
    hard_armed: bool,
}

impl IntraRecord {
    fn new(class: EdgeClass, tx: TxIdx, k: u16, peer: Option<TxIdx>) -> Self {
        let mut writers = Vec::new();
        let mut readers = Vec::new();
        match class {
            EdgeClass::Waw => {
                if let Some(p) = peer {
                    writers.push(p);
                }
                writers.push(tx);
            }
            EdgeClass::Raw | EdgeClass::War => {
                readers.push(tx);
                if let Some(p) = peer {
                    writers.push(p);
                }
            }
        }
        Self {
            last_class: class,
            recipe: Recipe::for_class(class),
            first_tx: tx,
            first_k: k,
            peer,
            writers,
            readers,
            retained: Vec::new(),
            ok_ema: 0.0,
            fail_ema: 0.0,
            hard_armed: false,
        }
    }

    fn credit(&self) -> f32 {
        let d = self.ok_ema + self.fail_ema + 1e-3;
        self.ok_ema / d
    }

    /// Detect on this access replaces the sticky recipe when the class differs.
    fn apply_detect(&mut self, class: EdgeClass, tx: TxIdx, k: u16, peer: Option<TxIdx>) -> Recipe {
        self.last_class = class;
        self.recipe = Recipe::for_class(class);
        if k < self.first_k {
            self.first_k = k;
            self.first_tx = tx;
        }
        if peer.is_some() {
            self.peer = peer;
        }
        match class {
            EdgeClass::Raw => push_unique(&mut self.readers, tx),
            EdgeClass::Waw => {
                if let Some(p) = peer {
                    push_unique(&mut self.writers, p);
                }
                push_unique(&mut self.writers, tx);
            }
            EdgeClass::War => {
                push_unique(&mut self.readers, tx);
                if let Some(p) = peer {
                    push_unique(&mut self.retained, (tx, p));
                }
            }
        }
        self.recipe
    }
}

fn push_unique<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) && v.len() < 96 {
        v.push(x);
    }
}

/// Cross-block radar. `action` is fixed to preheat-only.
#[derive(Debug, Clone)]
pub(crate) struct SpinePrior {
    /// Morph hint from the previous block. Radar only.
    pub morph: MorphClass,
    /// Previous block had a WAR edge.
    pub war_presence: bool,
    /// Mean relative edge-count delta. See SoT VolScore.
    pub vol_score: f32,
    /// Weak location weights. Never a fence list.
    pub loc_radar: Vec<(MemoryLocationHash, f32)>,
    /// Laplace mixture over recipes. Proposal only.
    pub recipe_proposal: [f32; 3],
    /// Page-Hinkley / CUSUM asked for a revoke on the last boundary.
    pub revoked: bool,
    /// Longest writer chain observed last block, ascending tx index.
    /// Radar for OrderedTip. Empty after revoke. Not an Avoid arm.
    pub chains: Vec<TxIdx>,
    /// Location of [`Self::chains`]. `u64::MAX` when there is no chain.
    pub chain_loc: MemoryLocationHash,
    ph_sum: f32,
    ph_min: f32,
    ph_n: u32,
    ph_mean: f32,
    cusum_pos: f32,
    cusum_neg: f32,
    prev_raw: u32,
    prev_waw: u32,
    prev_war: u32,
}

impl Default for SpinePrior {
    fn default() -> Self {
        Self {
            morph: MorphClass::Unknown,
            war_presence: false,
            vol_score: 0.0,
            loc_radar: Vec::new(),
            recipe_proposal: [0.0; 3],
            revoked: false,
            ph_sum: 0.0,
            ph_min: 0.0,
            ph_n: 0,
            ph_mean: 0.0,
            cusum_pos: 0.0,
            cusum_neg: 0.0,
            prev_raw: 0,
            prev_waw: 0,
            prev_war: 0,
            chains: Vec::new(),
            chain_loc: u64::MAX,
        }
    }
}

impl SpinePrior {
    /// Priors never arm Avoid.
    pub(crate) const fn action(&self) -> PriorAction {
        PriorAction::PreheatRadarOnly
    }

    /// A wrong prior must not be able to fence.
    pub(crate) const fn may_fence(&self) -> bool {
        false
    }
}

/// Counters copied out at `end_block` for the Soft=0 report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpineReport {
    /// Host accesses observed on the SpecFence path.
    pub access_events: usize,
    /// In-frame WaitTrueVersion calls that observed a true tip.
    pub yield_waits_ok: usize,
    /// Deadlock escapes that left the frame (must stay rare).
    pub yield_deadlocks: usize,
    /// RAW detects.
    pub raw_n: usize,
    /// WAW detects.
    pub waw_n: usize,
    /// WAR detects.
    pub war_n: usize,
    /// Locations hard-armed this block.
    pub armed_locs: usize,
    /// RetainHistory pins still held at end_block.
    pub retained_pins: usize,
    /// Prior revoked by VolScore / PH / CUSUM.
    pub prior_revoked: usize,
    /// `1` when the carried prior's action is PreheatRadarOnly.
    pub prior_radar_only: usize,
    /// Chain successors left on the queue until their pred has started.
    pub ordered_defer: usize,
    /// Next chain writer woken so it can enter and wait at the real read.
    pub ordered_handoff: usize,
    /// Aborted tips copied into the retain snapshot before Estimate.
    pub retain_keeps: usize,
    /// Ordered read whose pred tip was already published (no in-frame wait).
    pub tip_already: usize,
    /// Writer indexes carried on the ordered chain (0 when cold).
    pub chain_len: usize,
    /// High-water mark of concurrent spine owners. IdleStealWake keeps this ≤1.
    pub spine_cores_max: usize,
    /// Owners still held at `end_block` (leak if non-zero).
    pub spine_cores_end: usize,
    /// Handoff slot claims that entered execution.
    pub handoff_claims: usize,
    /// Second core refused because a spine owner was already live.
    pub claim_denied: usize,
    /// `unpark` of exactly one idle worker.
    pub exact_wakes: usize,
    /// Idle parks (ExactWakeToken, not a broadcast).
    pub idle_parks: usize,
    /// HelpRelease picks taken only after AdmitIndep was empty.
    pub help_releases: usize,
    /// Cross-core AdmitSteal hits (also mirrored on the metrics snapshot).
    pub steal_n: usize,
    /// Owner LIFO pops of `AdmitIndep`. Seeded work consumed locally.
    pub seed_owner_local_pops: usize,
    /// Sum across workers of thief `pop_top` time. Not a wall-clock interval.
    pub steal_top_ns: u64,
    /// Post-join `end_block` learn phase. Same window as `LearnReport::end_block_ns`.
    pub end_block_ns: u64,
    /// `exact_wake_one` ran and no core was parked.
    pub exact_wake_missed_nopark: usize,
    /// Scheduler idle entries.
    pub idle_spins: usize,
    /// Host wall from the SpecFence begin-block seed (prior sketch, arms,
    /// `admit_seed_begin_block`, crit install, `RunnableSet::seed_begin`)
    /// until the instant before `thread::scope`. Before any worker and before
    /// the chain span. Not PROFILE-gated.
    pub admit_seed_ns: u64,
    /// Host wall from the instant after the last worker spawn until
    /// `thread::scope` returns. Overlaps the worker tail (execute,
    /// post-exec validate, idle). Not additive with those sums.
    pub join_wait_ns: u64,
    /// Nanoseconds from the parallel-phase origin to the join-wait start.
    /// Intersect `[join_mark, join_mark + join_wait)` with focus
    /// `head_ms`/`tail_ms` (same origin) before treating join as tax.
    pub join_mark_origin_ns: u64,
    /// Sum across workers of the idle ring (heal + yield + spin + park).
    /// Includes [`Self::heal_ns`]. Not a wall clock.
    pub idle_ns: u64,
    /// Subset of [`Self::idle_ns`]: `heal_finished_preds`, sleeper wake,
    /// `heal`, `force_idle_recover`, and idle-arm `drain_wave`. Not yield,
    /// spin, or park.
    pub heal_ns: u64,
    /// Sum across workers of post-exec `drain_wave` + `validate_to_plan` +
    /// `resolve_plan::apply` (and the `Task::Validation` arm) whose window
    /// **start** is outside the prior-crit span
    /// `[head.first_start, tail.first_start)`. Start-classified: a window
    /// that begins inside the open span counts entirely as in-span.
    /// Not a wall clock. Not additive with [`Self::join_wait_ns`].
    pub post_exec_validate_ns: u64,
    /// Same post-exec window that started inside the open prior-crit span.
    /// This slice overlaps the span interval and is not tax by itself.
    /// `post_exec_validate_ns + post_exec_in_span_ns` is the unfiltered sum.
    pub post_exec_in_span_ns: u64,
    /// Prior crit head tx used by the span filter. `usize::MAX` when no chain.
    pub span_head: usize,
    /// Prior crit tail tx used by the span filter. `usize::MAX` when no chain.
    pub span_tail: usize,
}

/// Shared per-block spine. Workers share it. The Avoid table starts empty.
#[derive(Debug)]
pub(crate) struct AccessSpine {
    intra: DashMap<MemoryLocationHash, IntraRecord>,
    prior: SpinePrior,
    workers: usize,
    /// Hard-arm budget. Extra locations stay Admit + Detect.
    budget: usize,
    yield_waiters: AtomicUsize,
    escape_taken: AtomicBool,
    access_events: AtomicUsize,
    yield_ok: AtomicUsize,
    yield_deadlock: AtomicUsize,
    raw_n: AtomicUsize,
    waw_n: AtomicUsize,
    war_n: AtomicUsize,
    armed_n: AtomicUsize,
    /// Generation for the deadlock escape flag.
    escape_gen: AtomicU64,
    /// Previous block's longest chain. Not armed until a real write.
    ordered_loc: MemoryLocationHash,
    ordered_writers: Vec<TxIdx>,
    /// (reader, origin writer) bound by a real read. RetainHistory snapshots
    /// that origin when it would be replaced by Estimate.
    bounds: DashMap<MemoryLocationHash, Vec<(TxIdx, TxIdx)>>,
    published: DashSet<TxIdx>,
    started: DashSet<TxIdx>,
    handoff: AtomicUsize,
    ordered_defer: AtomicUsize,
    ordered_handoff: AtomicUsize,
    retain_keeps: AtomicUsize,
    tip_already: AtomicUsize,
    /// `usize::MAX` when no chain hop owns a core.
    spine_owner: AtomicUsize,
    spine_cores_max: AtomicUsize,
    handoff_claims: AtomicUsize,
    claim_denied: AtomicUsize,
}

impl AccessSpine {
    /// Empty Avoid. `prior` only warms the radar.
    pub(crate) fn begin(prior: SpinePrior, workers: usize) -> Self {
        let ordered_loc = prior.chain_loc;
        let ordered_writers = prior.chains.clone();
        Self {
            intra: DashMap::new(),
            prior,
            workers: workers.max(1),
            budget: 32,
            yield_waiters: AtomicUsize::new(0),
            escape_taken: AtomicBool::new(false),
            access_events: AtomicUsize::new(0),
            yield_ok: AtomicUsize::new(0),
            yield_deadlock: AtomicUsize::new(0),
            raw_n: AtomicUsize::new(0),
            waw_n: AtomicUsize::new(0),
            war_n: AtomicUsize::new(0),
            armed_n: AtomicUsize::new(0),
            escape_gen: AtomicU64::new(0),
            ordered_loc,
            ordered_writers,
            bounds: DashMap::new(),
            published: DashSet::new(),
            started: DashSet::new(),
            handoff: AtomicUsize::new(usize::MAX),
            ordered_defer: AtomicUsize::new(0),
            ordered_handoff: AtomicUsize::new(0),
            retain_keeps: AtomicUsize::new(0),
            tip_already: AtomicUsize::new(0),
            spine_owner: AtomicUsize::new(usize::MAX),
            spine_cores_max: AtomicUsize::new(0),
            handoff_claims: AtomicUsize::new(0),
            claim_denied: AtomicUsize::new(0),
        }
    }

    /// A chain hop already occupies a core. Callers must not start another.
    #[inline]
    pub(crate) fn spine_busy(&self) -> bool {
        self.spine_owner.load(Ordering::Acquire) != usize::MAX
    }

    /// Claim the single spine owner. Fails closed when another hop is live.
    #[inline]
    pub(crate) fn try_acquire(&self, tx: TxIdx) -> bool {
        match self
            .spine_owner
            .compare_exchange(usize::MAX, tx, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.spine_cores_max.fetch_max(1, Ordering::Relaxed);
                true
            }
            Err(cur) if cur == tx => true,
            Err(_) => {
                self.claim_denied.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Drop ownership when this hop finishes, blocks, or the claim is abandoned.
    #[inline]
    pub(crate) fn release_owner_tx(&self, tx: TxIdx) {
        let _ =
            self.spine_owner
                .compare_exchange(tx, usize::MAX, Ordering::AcqRel, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn note_handoff_claim(&self) {
        self.handoff_claims.fetch_add(1, Ordering::Relaxed);
    }

    /// Put `tx` in the slot only when it is empty. Does not count a new publish.
    #[inline]
    pub(crate) fn offer_handoff_if_absent(&self, tx: TxIdx) {
        self.restore_handoff(tx);
    }

    /// This tx is on the carried writer chain.
    pub(crate) fn is_ordered_member(&self, tx: TxIdx) -> bool {
        self.ordered_writers.binary_search(&tx).is_ok()
    }

    pub(crate) fn ordered_len(&self) -> usize {
        self.ordered_writers.len()
    }

    /// A chain successor may enter once its predecessor has started or finished.
    /// The head always may. Non-members always may (antichain fill).
    pub(crate) fn successor_blocked(
        &self,
        tx: TxIdx,
        pred_finished: impl Fn(TxIdx) -> bool,
    ) -> bool {
        let Ok(i) = self.ordered_writers.binary_search(&tx) else {
            return false;
        };
        if i == 0 {
            return false;
        }
        let pred = self.ordered_writers[i - 1];
        // Published or already finished. "Started" is not enough: waking the
        // whole tail at start pinned every core inside WaitTrueVersion.
        if self.published.contains(&pred) || pred_finished(pred) {
            return false;
        }
        true
    }

    pub(crate) fn note_ordered_defer(&self) {
        self.ordered_defer.fetch_add(1, Ordering::Relaxed);
    }

    /// Chain tx entered the interpreter. The successor is woken on publish,
    /// not here, so only one chain writer occupies a core.
    pub(crate) fn note_chain_start(&self, tx: TxIdx) {
        if self.ordered_writers.binary_search(&tx).is_ok() {
            self.started.insert(tx);
        }
    }

    /// Slot has no unclaimed spine hop. Does not take the hop.
    #[inline]
    pub(crate) fn handoff_is_empty(&self) -> bool {
        self.handoff.load(Ordering::Acquire) == usize::MAX
    }

    /// Successor to run next, if a chain start queued one.
    pub(crate) fn take_handoff(&self) -> Option<TxIdx> {
        let tx = self.handoff.swap(usize::MAX, Ordering::AcqRel);
        (tx != usize::MAX).then_some(tx)
    }

    /// Put a handoff back when the successor was still `ST_RUNNING`.
    pub(crate) fn restore_handoff(&self, tx: TxIdx) {
        let _ = self
            .handoff
            .compare_exchange(usize::MAX, tx, Ordering::AcqRel, Ordering::Relaxed);
    }

    /// Greatest chain writer below `tx` who has not published and is still open.
    pub(crate) fn ordered_blocker(
        &self,
        loc: MemoryLocationHash,
        tx: TxIdx,
        still_open: impl Fn(TxIdx) -> bool,
    ) -> Option<TxIdx> {
        if self.ordered_loc != loc || self.ordered_writers.is_empty() {
            return None;
        }
        self.ordered_writers
            .iter()
            .rev()
            .copied()
            .find(|&w| w < tx && !self.published.contains(&w) && still_open(w))
    }

    pub(crate) fn note_tip_already(&self) {
        self.tip_already.fetch_add(1, Ordering::Relaxed);
    }

    /// WAR pin: do not replace this writer's Data with Estimate.
    pub(crate) fn must_retain(&self, loc: MemoryLocationHash, writer: TxIdx) -> bool {
        self.intra
            .get(&loc)
            .is_some_and(|rec| rec.retained.iter().any(|&(_, w)| w == writer))
    }

    pub(crate) fn note_retain_keep(&self) {
        self.retain_keeps.fetch_add(1, Ordering::Relaxed);
    }

    fn mark_ordered_published(&self, loc: MemoryLocationHash, tx: TxIdx) {
        if self.ordered_loc != loc {
            return;
        }
        let Ok(i) = self.ordered_writers.binary_search(&tx) else {
            return;
        };
        self.published.insert(tx);
        let Some(&next) = self.ordered_writers.get(i + 1) else {
            return;
        };
        self.handoff.store(next, Ordering::Release);
        self.ordered_handoff.fetch_add(1, Ordering::Relaxed);
    }

    /// A host read bound `origin` as this reader's tip.
    pub(crate) fn note_reader_origin(&self, loc: MemoryLocationHash, reader: TxIdx, origin: TxIdx) {
        if origin >= reader {
            return;
        }
        let mut slot = self.bounds.entry(loc).or_default();
        if slot.iter().any(|&(r, o)| r == reader && o == origin) || slot.len() >= 128 {
            return;
        }
        slot.push((reader, origin));
    }

    /// WAR reader re-reading an origin that Estimate would otherwise drop.
    pub(crate) fn reader_needs_origin(
        &self,
        loc: MemoryLocationHash,
        reader: TxIdx,
        origin: TxIdx,
    ) -> bool {
        self.bounds
            .get(&loc)
            .is_some_and(|v| v.iter().any(|&(r, o)| r == reader && o == origin))
            && self
                .intra
                .get(&loc)
                .is_some_and(|rec| !rec.retained.is_empty())
    }

    /// Snapshot this origin before Estimate only when a WAR pin still needs it.
    pub(crate) fn should_pin_origin(&self, loc: MemoryLocationHash, origin: TxIdx) -> bool {
        self.bounds
            .get(&loc)
            .is_some_and(|v| v.iter().any(|&(_, o)| o == origin))
            && self
                .intra
                .get(&loc)
                .is_some_and(|rec| !rec.retained.is_empty())
    }

    /// Radar from the previous block. Opening a block does not copy it into Avoid.
    pub(crate) fn prior(&self) -> &SpinePrior {
        &self.prior
    }

    /// Host access. Cheap until a location is armed: one atomic.
    pub(crate) fn on_access(&self, ev: AccessEvent) {
        self.access_events.fetch_add(1, Ordering::Relaxed);
        if self.armed_n.load(Ordering::Relaxed) == 0 {
            return;
        }
        if let Some(mut rec) = self.intra.get_mut(&ev.loc) {
            // Touch keeps the record live. Class is applied by Detect, not here.
            let _ = rec.first_k;
            if ev.mode == AccessMode::Read {
                push_unique(&mut rec.readers, ev.tx);
            }
        }
    }

    /// RAW at a read: per-access Detect overrides any sticky recipe.
    pub(crate) fn detect_raw(&self, ev: AccessEvent, peer: TxIdx) -> Recipe {
        self.note_class(ev, EdgeClass::Raw, Some(peer))
    }

    /// WAW at a committed write.
    pub(crate) fn detect_waw(&self, ev: AccessEvent, peer: Option<TxIdx>) -> Recipe {
        self.note_class(ev, EdgeClass::Waw, peer)
    }

    /// WAR: pin `(reader, writer)` so the reader's tip stays in the record.
    pub(crate) fn detect_war(&self, ev: AccessEvent, reader: TxIdx, writer: TxIdx) -> Recipe {
        let recipe = self.note_class(
            AccessEvent { tx: reader, ..ev },
            EdgeClass::War,
            Some(writer),
        );
        debug_assert_eq!(recipe, Recipe::RetainHistory);
        recipe
    }

    fn note_class(&self, ev: AccessEvent, class: EdgeClass, peer: Option<TxIdx>) -> Recipe {
        match class {
            EdgeClass::Raw => {
                self.raw_n.fetch_add(1, Ordering::Relaxed);
            }
            EdgeClass::Waw => {
                self.waw_n.fetch_add(1, Ordering::Relaxed);
            }
            EdgeClass::War => {
                self.war_n.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut slot = self
            .intra
            .entry(ev.loc)
            .or_insert_with(|| IntraRecord::new(class, ev.tx, ev.k, peer));
        // Detect on this access. A different class replaces the sticky recipe.
        let recipe = slot.apply_detect(class, ev.tx, ev.k, peer);
        if !slot.hard_armed && self.armed_n.load(Ordering::Relaxed) < self.budget {
            slot.hard_armed = true;
            self.armed_n.fetch_add(1, Ordering::Relaxed);
        }
        recipe
    }

    /// Committed writes. Classifies WAW against known writers and WAR against known readers.
    pub(crate) fn on_write_effects(&self, tx: TxIdx, locs: &[MemoryLocationHash]) {
        for &loc in locs {
            let ev = AccessEvent {
                tx,
                loc,
                k: 0,
                depth: 0,
                mode: AccessMode::Write,
            };
            let peer = self
                .intra
                .get(&loc)
                .and_then(|rec| rec.writers.iter().copied().rev().find(|&w| w < tx));
            if peer.is_some() {
                self.detect_waw(ev, peer);
            } else if !self.intra.contains_key(&loc) {
                // First writer: record the prefix, no primitive yet.
                self.intra.insert(loc, {
                    let mut rec = IntraRecord::new(EdgeClass::Waw, tx, 0, None);
                    rec.writers.clear();
                    rec.writers.push(tx);
                    rec.hard_armed = false;
                    rec
                });
            } else {
                self.intra.alter(&loc, |_, mut rec| {
                    push_unique(&mut rec.writers, tx);
                    rec
                });
            }
            let readers: Vec<TxIdx> = self
                .intra
                .get(&loc)
                .map(|rec| rec.readers.iter().copied().filter(|&r| r < tx).collect())
                .unwrap_or_default();
            for reader in readers {
                self.detect_war(ev, reader, tx);
            }
            self.mark_ordered_published(loc, tx);
        }
    }

    /// True when this location's retained pins must survive a later write.
    pub(crate) fn retained_for(&self, loc: MemoryLocationHash) -> Vec<(TxIdx, TxIdx)> {
        self.intra
            .get(&loc)
            .map(|rec| rec.retained.clone())
            .unwrap_or_default()
    }

    /// In-frame wait entered. Returns false when this worker must release the core.
    pub(crate) fn enter_yield(&self) -> bool {
        let n = self.yield_waiters.fetch_add(1, Ordering::AcqRel) + 1;
        if n >= self.workers {
            // Last core would pin the producer. Caller unwinds one tx.
            self.yield_waiters.fetch_sub(1, Ordering::AcqRel);
            self.yield_deadlock.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Leave a yield wait that did not take the deadlock exit.
    pub(crate) fn leave_yield_ok(&self) {
        self.yield_waiters.fetch_sub(1, Ordering::AcqRel);
        self.yield_ok.fetch_add(1, Ordering::Relaxed);
    }

    /// Leave without counting a successful consume (timeout / abort path).
    pub(crate) fn leave_yield_deadlock(&self) {
        let prev = self.yield_waiters.fetch_sub(1, Ordering::AcqRel);
        if prev == 0 {
            self.yield_waiters.fetch_add(1, Ordering::AcqRel);
        }
        self.yield_deadlock.fetch_add(1, Ordering::Relaxed);
    }

    /// One worker may escape a stuck wait. Others keep the frame.
    pub(crate) fn try_escape(&self) -> bool {
        self.escape_taken
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Reset the escape flag so a later stall can free another core.
    pub(crate) fn clear_escape(&self) {
        self.escape_taken.store(false, Ordering::Release);
        self.escape_gen.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn yield_waiters(&self) -> usize {
        self.yield_waiters.load(Ordering::Relaxed)
    }

    pub(crate) fn workers(&self) -> usize {
        self.workers
    }

    /// EMA credit. Low credit does not switch primitives; Detect still owns the class.
    pub(crate) fn credit_ok(&self, loc: MemoryLocationHash) {
        self.bump_credit(loc, true);
    }

    /// EMA fail. Used when a wait gives the core back.
    pub(crate) fn credit_fail(&self, loc: MemoryLocationHash) {
        self.bump_credit(loc, false);
    }

    fn bump_credit(&self, loc: MemoryLocationHash, ok: bool) {
        const ETA: f32 = 0.2;
        if let Some(mut rec) = self.intra.get_mut(&loc) {
            if ok {
                rec.ok_ema = (1.0 - ETA) * rec.ok_ema + ETA;
            } else {
                rec.fail_ema = (1.0 - ETA) * rec.fail_ema + ETA;
            }
        }
    }

    /// Multi-location arm order. Higher score first. Does not fence by itself.
    pub(crate) fn score_loc(&self, loc: MemoryLocationHash) -> f32 {
        let Some(rec) = self.intra.get(&loc) else {
            return 0.0;
        };
        let n_w = rec.writers.len() as f32;
        let n_e = (rec.writers.len() + rec.readers.len()).max(1) as f32;
        let war = rec.retained.len() as f32 / n_e;
        let prior_w = self
            .prior
            .loc_radar
            .iter()
            .find(|(h, _)| *h == loc)
            .map(|(_, w)| *w)
            .unwrap_or(0.0);
        // Remaining chain proxy: writers already observed (L_rem grows as we learn).
        let l_rem = n_w;
        4.0 * l_rem + 2.0 * n_w + war + rec.credit() + 0.5 * prior_w
    }

    /// Close the block. Builds the next radar and clears Avoid (the map is dropped).
    pub(crate) fn end_block(&self) -> (SpineReport, SpinePrior) {
        let raw = self.raw_n.load(Ordering::Relaxed) as u32;
        let waw = self.waw_n.load(Ordering::Relaxed) as u32;
        let war = self.war_n.load(Ordering::Relaxed) as u32;
        let mut next = self.prior.clone();
        let (vol, storm) =
            vol_score_storm(next.prev_raw, next.prev_waw, next.prev_war, raw, waw, war);
        next.vol_score = vol;
        next.war_presence = war > 0;
        next.morph = morph_of(raw, waw, war);
        next.recipe_proposal = mixture_proposal(next.morph);
        let hit = radar_hit(&next.loc_radar, &self.intra);
        let war_delta = war as f32 - next.prev_war as f32;
        let ph = page_hinkley_step(&mut next, war_delta);
        let cu = cusum_step(&mut next, hit);
        next.revoked = storm || ph || cu;
        // Revoke drops predictive weights. The writer chain just observed in
        // this block is a fresh list, not the stale prior, so the next block
        // can still order it. A revoke with no chain clears the carried list.
        if next.revoked {
            next.loc_radar.clear();
        } else {
            next.loc_radar = self.hot_locs();
        }
        if let Some((loc, writers)) = self.longest_writer_chain() {
            next.chain_loc = loc;
            next.chains = writers;
        } else if next.revoked {
            next.chains.clear();
            next.chain_loc = u64::MAX;
        }
        next.prev_raw = raw;
        next.prev_waw = waw;
        next.prev_war = war;
        debug_assert!(!next.may_fence());
        let retained_pins = self.intra.iter().map(|r| r.retained.len()).sum();
        let report = SpineReport {
            access_events: self.access_events.load(Ordering::Relaxed),
            yield_waits_ok: self.yield_ok.load(Ordering::Relaxed),
            yield_deadlocks: self.yield_deadlock.load(Ordering::Relaxed),
            raw_n: raw as usize,
            waw_n: waw as usize,
            war_n: war as usize,
            armed_locs: self.armed_n.load(Ordering::Relaxed),
            retained_pins,
            prior_revoked: usize::from(next.revoked),
            prior_radar_only: 1,
            ordered_defer: self.ordered_defer.load(Ordering::Relaxed),
            ordered_handoff: self.ordered_handoff.load(Ordering::Relaxed),
            retain_keeps: self.retain_keeps.load(Ordering::Relaxed),
            tip_already: self.tip_already.load(Ordering::Relaxed),
            chain_len: self.ordered_writers.len(),
            spine_cores_max: self.spine_cores_max.load(Ordering::Relaxed),
            spine_cores_end: usize::from(self.spine_owner.load(Ordering::Acquire) != usize::MAX),
            handoff_claims: self.handoff_claims.load(Ordering::Relaxed),
            claim_denied: self.claim_denied.load(Ordering::Relaxed),
            exact_wakes: 0,
            idle_parks: 0,
            help_releases: 0,
            steal_n: 0,
            seed_owner_local_pops: 0,
            steal_top_ns: 0,
            end_block_ns: 0,
            exact_wake_missed_nopark: 0,
            idle_spins: 0,
            admit_seed_ns: 0,
            join_wait_ns: 0,
            join_mark_origin_ns: 0,
            idle_ns: 0,
            heal_ns: 0,
            post_exec_validate_ns: 0,
            post_exec_in_span_ns: 0,
            span_head: usize::MAX,
            span_tail: usize::MAX,
        };
        (report, next)
    }

    /// Longest writer list this block. Chain order is ascending tx index.
    fn longest_writer_chain(&self) -> Option<(MemoryLocationHash, Vec<TxIdx>)> {
        let mut best: Option<(MemoryLocationHash, Vec<TxIdx>)> = None;
        for rec in self.intra.iter() {
            if rec.writers.len() < 2 {
                continue;
            }
            let mut writers = rec.writers.clone();
            writers.sort_unstable();
            writers.dedup();
            if best.as_ref().is_none_or(|(_, w)| writers.len() > w.len()) {
                best = Some((*rec.key(), writers));
            }
        }
        best
    }

    fn hot_locs(&self) -> Vec<(MemoryLocationHash, f32)> {
        let mut v: Vec<(MemoryLocationHash, f32)> = self
            .intra
            .iter()
            .map(|r| {
                let heat = (r.writers.len() + r.readers.len()) as f32;
                (*r.key(), heat)
            })
            .filter(|(_, h)| *h > 0.0)
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(16);
        // Decay toward the new observation (λ in (0.8, 0.95) is the carried weight).
        const LAMBDA: f32 = 0.9;
        for (loc, w) in &mut v {
            let prev = self
                .prior
                .loc_radar
                .iter()
                .find(|(h, _)| h == loc)
                .map(|(_, p)| *p)
                .unwrap_or(0.0);
            *w = LAMBDA * prev + 1.0;
        }
        v
    }
}

/// `max(L_crit, sum_work / cores)`.
pub(crate) fn ideal_lb(l_crit: f64, sum_work: f64, cores: f64) -> f64 {
    let parallel = if cores > 0.0 {
        sum_work / cores
    } else {
        sum_work
    };
    l_crit.max(parallel)
}

/// VolScore and the hard storm-to-quiet revoke.
pub(crate) fn vol_score_storm(
    raw_i: u32,
    waw_i: u32,
    war_i: u32,
    raw_j: u32,
    waw_j: u32,
    war_j: u32,
) -> (f32, bool) {
    let rel = |a: u32, b: u32| {
        let d = (a as i32 - b as i32).unsigned_abs() as f32;
        d / (a.max(1) as f32)
    };
    let vol = (rel(raw_i, raw_j) + rel(waw_i, waw_j) + rel(war_i, war_j)) / 3.0;
    let sum_i = raw_i + waw_i + war_i;
    let sum_j = raw_j + waw_j + war_j;
    let storm = sum_i >= 80 && sum_j <= 5;
    (vol, storm)
}

fn morph_of(raw: u32, waw: u32, war: u32) -> MorphClass {
    let sum = raw + waw + war;
    if sum <= 5 {
        return MorphClass::NearIndependent;
    }
    if sum < 40 && raw.max(waw).max(war) * 2 < sum {
        return MorphClass::SparseConflict;
    }
    if raw > waw.saturating_mul(2) && raw > war.saturating_mul(2) {
        return MorphClass::RawFanOut;
    }
    if waw > raw && waw >= war {
        return MorphClass::WawSpine;
    }
    MorphClass::MixedRawWaw
}

/// Laplace P(class | morph) from the v1.1 table, as a recipe proposal.
pub(crate) fn laplace_recipe(morph: MorphClass) -> [f32; 3] {
    // Order: WaitTrueVersion, OrderedTip, RetainHistory.
    match morph {
        MorphClass::RawFanOut => [0.675, 0.173, 0.152],
        MorphClass::MixedRawWaw => [0.414, 0.345, 0.242],
        MorphClass::WawSpine => [0.369, 0.352, 0.279],
        MorphClass::SparseConflict => [0.500, 0.375, 0.125],
        MorphClass::NearIndependent => [0.400, 0.400, 0.200],
        MorphClass::Unknown => [0.0, 0.0, 0.0],
    }
}

/// Mixture of the morph proposal with a flat "no proposal" residue.
/// The residue keeps the weights from becoming a command.
fn mixture_proposal(morph: MorphClass) -> [f32; 3] {
    let p = laplace_recipe(morph);
    if p == [0.0, 0.0, 0.0] {
        return p;
    }
    const PI_M: f32 = 0.5;
    [p[0] * PI_M, p[1] * PI_M, p[2] * PI_M]
}

fn page_hinkley_step(prior: &mut SpinePrior, x: f32) -> bool {
    // δ = 0.5, λ = 8.
    // The cumulative form flags a rise off the minimum. A one-step crash
    // (Δwar = -17) is a shock against the mean held *before* this sample;
    // the running-mean form alone absorbs that point and never crosses λ.
    const DELTA: f32 = 0.5;
    const LAMBDA: f32 = 8.0;
    let shock = prior.ph_mean - x;
    prior.ph_n = prior.ph_n.saturating_add(1);
    let n = prior.ph_n as f32;
    prior.ph_mean += (x - prior.ph_mean) / n;
    prior.ph_sum += x - prior.ph_mean - DELTA;
    if prior.ph_sum < prior.ph_min {
        prior.ph_min = prior.ph_sum;
    }
    shock > LAMBDA || prior.ph_sum - prior.ph_min > LAMBDA
}

fn cusum_step(prior: &mut SpinePrior, hit: f32) -> bool {
    // μ0 = 0.26 (mean hot-ℓ hit). λ = 0.5. A 0.80 hit does not revoke; a 0 hit can.
    const MU: f32 = 0.26;
    const DELTA: f32 = 0.05;
    const LAMBDA: f32 = 0.5;
    prior.cusum_pos = (prior.cusum_pos + (hit - MU - DELTA)).max(0.0);
    prior.cusum_neg = (prior.cusum_neg - (hit - MU + DELTA)).max(0.0);
    prior.cusum_pos > LAMBDA || prior.cusum_neg > LAMBDA
}

fn radar_hit(
    radar: &[(MemoryLocationHash, f32)],
    intra: &DashMap<MemoryLocationHash, IntraRecord>,
) -> f32 {
    if radar.is_empty() {
        return 0.0;
    }
    let hit = radar
        .iter()
        .filter(|(loc, _)| intra.contains_key(loc))
        .count();
    hit as f32 / radar.len() as f32
}

/// Handler policy: YieldWait must not discard the frame. Other DB errors do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandlerFault {
    /// In-frame wait. Journal and frame stay.
    YieldWait,
    /// Fatal database error. Discard.
    Fatal,
}

pub(crate) fn handler_preserves_frame(fault: HandlerFault) -> bool {
    matches!(fault, HandlerFault::YieldWait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_overrides_sticky_recipe_on_same_loc() {
        let spine = AccessSpine::begin(SpinePrior::default(), 4);
        let loc = 7u64;
        let read = AccessEvent {
            tx: 4,
            loc,
            k: 3,
            depth: 1,
            mode: AccessMode::Read,
        };
        assert_eq!(spine.detect_raw(read, 1), Recipe::WaitTrueVersion);
        let write = AccessEvent {
            tx: 9,
            loc,
            k: 8,
            depth: 1,
            mode: AccessMode::Write,
        };
        assert_eq!(spine.detect_waw(write, Some(4)), Recipe::OrderedTip);
        let again = AccessEvent {
            tx: 12,
            loc,
            k: 2,
            depth: 2,
            mode: AccessMode::Read,
        };
        assert_eq!(spine.detect_raw(again, 9), Recipe::WaitTrueVersion);
        let rec = spine.intra.get(&loc).unwrap();
        assert_eq!(rec.last_class, EdgeClass::Raw);
        assert_eq!(rec.recipe, Recipe::WaitTrueVersion);
        assert!(rec.writers.contains(&4) && rec.writers.contains(&9));
    }

    #[test]
    fn prior_never_fences_and_storm_revokes_radar() {
        let mut prior = SpinePrior::default();
        prior.prev_raw = 36;
        prior.prev_waw = 30;
        prior.prev_war = 17;
        prior.loc_radar.push((99, 1.0));
        let spine = AccessSpine::begin(prior, 2);
        // Quiet block: no detects. storm_to_quiet (83 → 0) clears the radar.
        let (report, next) = spine.end_block();
        assert!(next.revoked);
        assert!(next.loc_radar.is_empty());
        assert!(!next.may_fence());
        assert_eq!(next.action(), PriorAction::PreheatRadarOnly);
        assert_eq!(report.prior_radar_only, 1);
        assert_eq!(report.armed_locs, 0);
    }

    #[test]
    fn warm_pair_keeps_radar_and_laplace_is_proposal_only() {
        let mut prior = SpinePrior::default();
        prior.loc_radar = vec![(11, 1.0), (12, 1.0), (13, 1.0), (14, 1.0), (15, 1.0)];
        prior.prev_raw = 10;
        prior.prev_waw = 8;
        prior.prev_war = 4;
        let spine = AccessSpine::begin(prior, 2);
        for (tx, loc) in [(1, 11u64), (2, 12), (3, 13), (4, 14)] {
            spine.detect_raw(
                AccessEvent {
                    tx,
                    loc,
                    k: 1,
                    depth: 1,
                    mode: AccessMode::Read,
                },
                0,
            );
        }
        let (_r, next) = spine.end_block();
        assert!(!next.revoked, "hit 0.8 must not revoke");
        assert!(!next.loc_radar.is_empty());
        let prop = laplace_recipe(MorphClass::RawFanOut);
        assert!((prop[0] - 0.675).abs() < 1e-6);
        // Mixture is half mass: not a hard assignment.
        let mix = mixture_proposal(MorphClass::RawFanOut);
        assert!(mix[0] < prop[0]);
        assert!(!next.may_fence());
    }

    #[test]
    fn retain_history_pin_survives_later_write_record() {
        let spine = AccessSpine::begin(SpinePrior::default(), 2);
        let loc = 42u64;
        spine.detect_raw(
            AccessEvent {
                tx: 3,
                loc,
                k: 1,
                depth: 1,
                mode: AccessMode::Read,
            },
            1,
        );
        spine.on_write_effects(8, &[loc]);
        let pins = spine.retained_for(loc);
        assert!(
            pins.iter().any(|&(r, w)| r == 3 && w == 8),
            "reader tip must stay pinned, got {pins:?}"
        );
    }

    #[test]
    fn multi_loc_budget_and_score_prefer_longer_writer_chain() {
        let spine = AccessSpine::begin(SpinePrior::default(), 4);
        for tx in 0..6 {
            spine.detect_waw(
                AccessEvent {
                    tx,
                    loc: 1,
                    k: tx as u16,
                    depth: 1,
                    mode: AccessMode::Write,
                },
                if tx == 0 { None } else { Some(tx - 1) },
            );
        }
        spine.detect_waw(
            AccessEvent {
                tx: 2,
                loc: 2,
                k: 1,
                depth: 1,
                mode: AccessMode::Write,
            },
            Some(1),
        );
        assert!(spine.score_loc(1) > spine.score_loc(2));
        assert!(spine.armed_n.load(Ordering::Relaxed) >= 2);
    }

    #[test]
    fn ideal_lb_is_not_n_over_cores() {
        let lb = ideal_lb(17.0, 152.0, 8.0);
        assert!((lb - 19.0).abs() < 1e-9);
        // n/C for a 176-tx block is a different number and is not the bound.
        let n_over_c = 176.0 / 8.0;
        assert!(n_over_c > lb);
    }

    #[test]
    fn handler_yield_preserves_frame_policy() {
        assert!(handler_preserves_frame(HandlerFault::YieldWait));
        assert!(!handler_preserves_frame(HandlerFault::Fatal));
    }

    #[test]
    fn page_hinkley_large_drop_revokes() {
        let mut prior = SpinePrior::default();
        // One large negative WAR step (14689598→99 style, Δwar = -17).
        let revoke = page_hinkley_step(&mut prior, -17.0);
        assert!(revoke);
    }

    #[test]
    fn spine_owner_stays_one() {
        let spine = AccessSpine::begin(SpinePrior::default(), 4);
        assert!(spine.try_acquire(3));
        assert!(!spine.try_acquire(4), "second hop must not share the core");
        assert!(spine.spine_busy());
        spine.release_owner_tx(3);
        assert!(spine.try_acquire(4));
        spine.release_owner_tx(4);
        assert!(!spine.spine_busy());
    }

    #[test]
    fn ordered_successor_waits_for_pred_start_then_handoff() {
        let mut prior = SpinePrior::default();
        prior.chain_loc = 7;
        prior.chains = vec![1, 4, 9];
        let spine = AccessSpine::begin(prior, 4);
        assert_eq!(
            spine.armed_n.load(Ordering::Relaxed),
            0,
            "chain is not an Avoid arm"
        );
        assert!(spine.successor_blocked(4, |_| false));
        assert!(!spine.successor_blocked(1, |_| false));
        spine.note_chain_start(1);
        assert!(spine.successor_blocked(4, |_| false));
        assert_eq!(spine.take_handoff(), None);
        assert_eq!(spine.ordered_blocker(7, 4, |_| true), Some(1));
        spine.on_write_effects(1, &[7]);
        assert!(!spine.successor_blocked(4, |_| false));
        assert_eq!(spine.take_handoff(), Some(4));
        assert_eq!(spine.ordered_blocker(7, 4, |_| true), None);
    }

    #[test]
    fn war_pin_is_a_retain_keep() {
        let spine = AccessSpine::begin(SpinePrior::default(), 2);
        let read = AccessEvent {
            tx: 3,
            loc: 1,
            k: 1,
            depth: 1,
            mode: AccessMode::Read,
        };
        spine.detect_raw(read, 1);
        spine.on_write_effects(8, &[1]);
        assert!(spine.must_retain(1, 8));
        spine.note_reader_origin(1, 3, 1);
        assert!(spine.should_pin_origin(1, 1));
        assert!(spine.reader_needs_origin(1, 3, 1));
        assert!(
            !spine.should_pin_origin(1, 8),
            "the later writer is not the historical tip"
        );
        spine.note_retain_keep();
        let (report, next) = spine.end_block();
        assert!(report.retain_keeps >= 1);
        assert!(!next.may_fence());
    }

    #[test]
    fn soft0_begin_has_empty_avoid_even_with_radar() {
        let mut prior = SpinePrior::default();
        prior.loc_radar.push((123, 4.0));
        prior.recipe_proposal = laplace_recipe(MorphClass::WawSpine);
        let spine = AccessSpine::begin(prior, 8);
        assert_eq!(spine.armed_n.load(Ordering::Relaxed), 0);
        assert!(spine.intra.is_empty());
        assert!(!spine.prior().may_fence());
    }
}
