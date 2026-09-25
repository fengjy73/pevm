//! Soft0-RegionLearnAvoid v2.
//!
//! Off-spine hot locations only. Spine locations stay on the ordered Handoff.
//! A transaction that is not a predicted toucher pays one false flag on its
//! own `VmDb` and does not touch this table. Radar seeds the next block; the
//! location is armed on the first in-block evidence, then drained when its
//! last predicted writer publishes.
//!
//! `SPECFENCE_REGION_LEARN_AVOID_V2=0` builds a disabled table. Disabled
//! methods return before any map, lock, or flag write.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use smallvec::SmallVec;

use crate::{MemoryLocationHash, TxIdx};

use super::sf_mv::SfTipTable;

const FLAG_ARMED: u8 = 2;
const MAX_SLOTS: usize = 12;
const QUIET_EXIT: u8 = 3;
const EMPTY_LOC: u64 = u64::MAX;

/// Cross-block radar. Not an Avoid arm. Block-open leaves every slot disarmed.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionRadarPrior {
    pub(crate) seeds: Vec<RegionSeed>,
}

/// One location the previous block replayed or protected.
#[derive(Debug, Clone)]
pub(crate) struct RegionSeed {
    pub(crate) loc: MemoryLocationHash,
    pub(crate) writers: Vec<TxIdx>,
    pub(crate) readers: Vec<TxIdx>,
    pub(crate) quiet: u8,
    pub(crate) full_hits: u32,
}

/// What the access does after the predecessor state is known. No Estimate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PredState {
    /// No lower writer on this edge.
    None,
    /// Lower writer already published true Data or is done.
    Published,
    /// Lower writer is executing or holds an SF version tip.
    Live,
    /// Lower writer has not been picked yet.
    Unstarted,
    /// Lower writer started and left without a true publish.
    Left,
}

/// Edge op at the true tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionOp {
    /// Read the published value, or there is no edge.
    Pass,
    /// Predecessor has not started. Detect stays on; do not pin a core.
    Unstarted,
    /// Reader keeps the version it binds. Neither side waits.
    Retain,
    /// Wait one hop. `ordered` is the non-spine WAW access-point hop.
    Wait { ordered: bool },
}

#[derive(Debug)]
struct Slot {
    loc: MemoryLocationHash,
    writers: Vec<TxIdx>,
    touchers: Vec<TxIdx>,
    armed: AtomicBool,
    drained: AtomicBool,
    armed_at: AtomicUsize,
    waits: AtomicUsize,
    ordered: AtomicUsize,
    passes: AtomicUsize,
    unstarted: AtomicUsize,
    retains: AtomicUsize,
    fulls: AtomicUsize,
    wait_ns: AtomicU64,
    resolves: AtomicUsize,
    published: Mutex<Vec<TxIdx>>,
    /// Predicted writers that finished this block without a write of `loc`.
    skipped: Mutex<Vec<TxIdx>>,
}

impl Slot {
    fn new(loc: MemoryLocationHash, writers: Vec<TxIdx>, touchers: Vec<TxIdx>) -> Self {
        Self {
            loc,
            writers,
            touchers,
            armed: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            armed_at: AtomicUsize::new(usize::MAX),
            waits: AtomicUsize::new(0),
            ordered: AtomicUsize::new(0),
            passes: AtomicUsize::new(0),
            unstarted: AtomicUsize::new(0),
            retains: AtomicUsize::new(0),
            fulls: AtomicUsize::new(0),
            wait_ns: AtomicU64::new(0),
            resolves: AtomicUsize::new(0),
            published: Mutex::new(Vec::new()),
            skipped: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Debug)]
struct Obs {
    loc: MemoryLocationHash,
    readers: Vec<TxIdx>,
    writers: Vec<TxIdx>,
    full_n: u32,
    resolve_n: u32,
}

/// Shared per-block region table. Workers share it. Avoid starts empty.
#[derive(Debug)]
pub(crate) struct RegionAvoid {
    enabled: bool,
    block_size: usize,
    /// Crit chain location. Region never arms it.
    spine_crit: MemoryLocationHash,
    /// Ordered spine location. Same delegation.
    spine_ordered: MemoryLocationHash,
    beneficiary: MemoryLocationHash,
    flags: Vec<std::sync::atomic::AtomicU8>,
    /// 0 = not a toucher, 1 = predicted reader, 2 = predicted writer.
    roles: Vec<u8>,
    probes: Vec<SmallVec<[MemoryLocationHash; 2]>>,
    slots: Mutex<Vec<Arc<Slot>>>,
    armed_locs: [AtomicU64; MAX_SLOTS],
    armed_n: AtomicUsize,
    any_armed: AtomicBool,
    in_frame: AtomicUsize,
    nontoucher_probes: AtomicUsize,
    full_armed: AtomicUsize,
    full_unarmed: AtomicUsize,
    observed: Mutex<Vec<Obs>>,
}

/// Per-tx bind copied onto `VmDb` at dispatch. `on == false` is the fast path.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RegionBind {
    pub(crate) on: bool,
    pub(crate) writer: bool,
    pub(crate) locs: [MemoryLocationHash; 4],
    pub(crate) nlocs: u8,
}

impl RegionBind {
    fn off() -> Self {
        Self {
            on: false,
            writer: false,
            locs: [EMPTY_LOC; 4],
            nlocs: 0,
        }
    }
}

/// Armed-slot view for one access. Counters live on the slot.
pub(crate) struct RegionView {
    slot: Arc<Slot>,
    is_writer: bool,
    higher_writer: bool,
}

impl RegionView {
    pub(crate) fn decide(&self, state: PredState) -> RegionOp {
        if self.slot.drained.load(Ordering::Relaxed) {
            return RegionOp::Pass;
        }
        match state {
            PredState::Live | PredState::Left => RegionOp::Wait {
                ordered: self.is_writer,
            },
            PredState::Unstarted => RegionOp::Unstarted,
            PredState::Published | PredState::None => {
                if !self.is_writer && self.higher_writer {
                    RegionOp::Retain
                } else {
                    RegionOp::Pass
                }
            }
        }
    }

    pub(crate) fn note(&self, op: RegionOp, wait_ns: u64) {
        match op {
            RegionOp::Pass => {
                self.slot.passes.fetch_add(1, Ordering::Relaxed);
            }
            RegionOp::Unstarted => {
                self.slot.unstarted.fetch_add(1, Ordering::Relaxed);
            }
            RegionOp::Retain => {
                self.slot.retains.fetch_add(1, Ordering::Relaxed);
            }
            RegionOp::Wait { ordered } => {
                self.slot.waits.fetch_add(1, Ordering::Relaxed);
                if ordered {
                    self.slot.ordered.fetch_add(1, Ordering::Relaxed);
                }
                if wait_ns > 0 {
                    self.slot.wait_ns.fetch_add(wait_ns, Ordering::Relaxed);
                }
            }
        }
    }

    /// Nearest lower predicted writer who has not published this location.
    /// A done writer with no data is not a floor. `extra` is a live open
    /// writer observed outside the predicted list; it wins only when it sits
    /// strictly above any real data floor and closer than the predicted hop.
    pub(crate) fn pick_pred(
        &self,
        tx: TxIdx,
        wrote: impl Fn(TxIdx) -> bool,
        done: impl Fn(TxIdx) -> bool,
        extra: Option<TxIdx>,
    ) -> Option<TxIdx> {
        let i = self.slot.writers.partition_point(|&w| w < tx);
        let extra = extra.filter(|&e| e < tx && !done(e) && !wrote(e));
        for &w in self.slot.writers[..i].iter().rev() {
            if wrote(w) {
                return extra.filter(|&e| e > w);
            }
            if !done(w) {
                return match extra {
                    Some(e) if e > w => Some(e),
                    _ => Some(w),
                };
            }
        }
        extra
    }
}

impl RegionAvoid {
    pub(crate) fn disabled() -> Self {
        Self::from_parts(
            false,
            0,
            RegionRadarPrior::default(),
            EMPTY_LOC,
            EMPTY_LOC,
            EMPTY_LOC,
        )
    }

    /// Install the radar. No location is armed. Flag-off returns [`Self::disabled`].
    pub(crate) fn begin(
        specfence: bool,
        block_size: usize,
        prior: &RegionRadarPrior,
        spine_crit: MemoryLocationHash,
        spine_ordered: MemoryLocationHash,
        beneficiary: MemoryLocationHash,
    ) -> Self {
        if !specfence || !flag_enabled() {
            return Self::disabled();
        }
        Self::from_parts(
            true,
            block_size,
            prior.clone(),
            spine_crit,
            spine_ordered,
            beneficiary,
        )
    }

    fn from_parts(
        enabled: bool,
        block_size: usize,
        prior: RegionRadarPrior,
        spine_crit: MemoryLocationHash,
        spine_ordered: MemoryLocationHash,
        beneficiary: MemoryLocationHash,
    ) -> Self {
        let mut flags = Vec::new();
        let mut roles = Vec::new();
        let mut probes = Vec::new();
        if enabled && block_size > 0 {
            flags.resize_with(block_size, || std::sync::atomic::AtomicU8::new(0));
            roles.resize(block_size, 0);
            probes.resize(block_size, SmallVec::new());
        }
        let mut table = Self {
            enabled,
            block_size,
            spine_crit,
            spine_ordered,
            beneficiary,
            flags,
            roles,
            probes,
            slots: Mutex::new(Vec::new()),
            armed_locs: std::array::from_fn(|_| AtomicU64::new(EMPTY_LOC)),
            armed_n: AtomicUsize::new(0),
            any_armed: AtomicBool::new(false),
            in_frame: AtomicUsize::new(0),
            nontoucher_probes: AtomicUsize::new(0),
            full_armed: AtomicUsize::new(0),
            full_unarmed: AtomicUsize::new(0),
            observed: Mutex::new(Vec::new()),
        };
        if enabled {
            for seed in prior.seeds {
                if table.excluded(seed.loc) {
                    continue;
                }
                if seed.writers.len() >= 32 {
                    continue;
                }
                table.insert_slot(seed.loc, seed.writers, seed.readers);
            }
        }
        table
    }

    #[inline]
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub(crate) fn any_armed(&self) -> bool {
        self.enabled && self.any_armed.load(Ordering::Acquire)
    }

    #[inline]
    pub(crate) fn loc_armed(&self, loc: MemoryLocationHash) -> bool {
        self.enabled && self.armed_has(loc)
    }

    /// Dispatch bind. Non-touchers get `on: false` and never re-enter.
    pub(crate) fn bind(&self, tx: TxIdx) -> RegionBind {
        if !self.enabled || tx >= self.roles.len() || self.roles[tx] == 0 {
            return RegionBind::off();
        }
        let mut locs = [EMPTY_LOC; 4];
        let mut nlocs = 0u8;
        if self.roles[tx] == 2 {
            for &loc in &self.probes[tx] {
                if nlocs == 4 {
                    break;
                }
                locs[nlocs as usize] = loc;
                nlocs += 1;
            }
        }
        RegionBind {
            on: true,
            writer: self.roles[tx] == 2,
            locs,
            nlocs,
        }
    }

    fn excluded(&self, loc: MemoryLocationHash) -> bool {
        loc == self.beneficiary
            || (self.spine_crit != EMPTY_LOC && loc == self.spine_crit)
            || (self.spine_ordered != EMPTY_LOC && loc == self.spine_ordered)
    }

    fn insert_slot(&mut self, loc: MemoryLocationHash, writers: Vec<TxIdx>, readers: Vec<TxIdx>) {
        if self.excluded(loc) || writers.len() >= 32 {
            return;
        }
        let mut slots = self.slots.lock().unwrap();
        if slots.iter().any(|s| s.loc == loc) || slots.len() >= MAX_SLOTS {
            return;
        }
        let mut touchers = writers.clone();
        touchers.extend(readers.iter().copied());
        touchers.sort_unstable();
        touchers.dedup();
        let mut writers = writers;
        writers.sort_unstable();
        writers.dedup();
        for &tx in &touchers {
            if tx >= self.block_size {
                continue;
            }
            if writers.binary_search(&tx).is_ok() {
                self.roles[tx] = 2;
                let probe = &mut self.probes[tx];
                if !probe.contains(&loc) && probe.len() < 4 {
                    probe.push(loc);
                }
            } else if self.roles[tx] == 0 {
                self.roles[tx] = 1;
            }
        }
        slots.push(Arc::new(Slot::new(loc, writers, touchers)));
    }

    fn armed_has(&self, loc: MemoryLocationHash) -> bool {
        let n = self.armed_n.load(Ordering::Acquire).min(MAX_SLOTS);
        for i in 0..n {
            if self.armed_locs[i].load(Ordering::Acquire) == loc {
                return true;
            }
        }
        false
    }

    fn slot(&self, loc: MemoryLocationHash) -> Option<Arc<Slot>> {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.loc == loc)
            .cloned()
    }

    /// E1: a predicted writer has begun accessing `loc`. Arms that radar slot.
    pub(crate) fn try_arm(&self, tx: TxIdx, loc: MemoryLocationHash) {
        if !self.enabled || self.excluded(loc) {
            return;
        }
        let Some(slot) = self.slot(loc) else {
            return;
        };
        if slot.writers.binary_search(&tx).is_err() {
            return;
        }
        self.arm_slot(&slot, tx);
    }

    fn arm_slot(&self, slot: &Slot, evidence_tx: TxIdx) {
        if slot
            .armed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        slot.armed_at.store(evidence_tx, Ordering::Relaxed);
        for &t in &slot.touchers {
            if t > evidence_tx && t < self.flags.len() {
                self.flags[t].store(FLAG_ARMED, Ordering::Release);
            }
        }
        let i = self.armed_n.fetch_add(1, Ordering::AcqRel);
        if i < MAX_SLOTS {
            self.armed_locs[i].store(slot.loc, Ordering::Release);
        }
        self.any_armed.store(true, Ordering::Release);
    }

    /// E2: an existing peek already saw an unpublished lower writer.
    pub(crate) fn note_raw_evidence(&self, tx: TxIdx, loc: MemoryLocationHash, writer: TxIdx) {
        if !self.enabled || self.excluded(loc) || writer >= tx {
            return;
        }
        self.observe(loc, Some(tx), Some(writer), false, false);
        if let Some(slot) = self.slot(loc) {
            self.arm_slot(&slot, tx.min(writer));
        }
    }

    /// E3: validation failed on `loc`. The resolving transaction is a toucher.
    pub(crate) fn on_resolve(
        &self,
        loc: MemoryLocationHash,
        tx: TxIdx,
        full: bool,
        peer: Option<TxIdx>,
    ) {
        if !self.enabled || self.excluded(loc) {
            return;
        }
        let was_armed = self.armed_has(loc);
        if full {
            if was_armed {
                self.full_armed.fetch_add(1, Ordering::Relaxed);
                if let Some(slot) = self.slot(loc) {
                    slot.fulls.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                self.full_unarmed.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.observe(loc, Some(tx), peer.filter(|&w| w < tx), full, true);
        if let Some(slot) = self.slot(loc) {
            slot.resolves.fetch_add(1, Ordering::Relaxed);
            if !was_armed {
                self.arm_slot(&slot, tx);
            }
        }
    }

    fn observe(
        &self,
        loc: MemoryLocationHash,
        reader: Option<TxIdx>,
        writer: Option<TxIdx>,
        full: bool,
        resolve: bool,
    ) {
        if self.excluded(loc) {
            return;
        }
        let mut obs = self.observed.lock().unwrap();
        if !obs.iter().any(|e| e.loc == loc) {
            if obs.len() >= 64 {
                return;
            }
            obs.push(Obs {
                loc,
                readers: Vec::new(),
                writers: Vec::new(),
                full_n: 0,
                resolve_n: 0,
            });
        }
        let entry = obs.iter_mut().find(|e| e.loc == loc).unwrap();
        if let Some(r) = reader
            && !entry.readers.contains(&r)
            && entry.readers.len() < 48
        {
            entry.readers.push(r);
        }
        if let Some(w) = writer
            && !entry.writers.contains(&w)
            && entry.writers.len() < 48
        {
            entry.writers.push(w);
        }
        if resolve {
            entry.resolve_n = entry.resolve_n.saturating_add(1);
        }
        if full {
            entry.full_n = entry.full_n.saturating_add(1);
        }
    }

    /// Access-point view. `None` when this tx should pass without a slot lock
    /// beyond the armed-loc scan (not a toucher of an armed region).
    pub(crate) fn view(&self, tx: TxIdx, loc: MemoryLocationHash) -> Option<RegionView> {
        if !self.enabled || !self.any_armed() || self.excluded(loc) {
            return None;
        }
        let flagged = tx < self.flags.len() && self.flags[tx].load(Ordering::Acquire) == FLAG_ARMED;
        let writer_probe =
            tx < self.roles.len() && self.roles[tx] == 2 && self.probes[tx].contains(&loc);
        if !flagged && !writer_probe {
            if tx < self.roles.len() && self.roles[tx] == 0 && self.armed_has(loc) {
                self.nontoucher_probes.fetch_add(1, Ordering::Relaxed);
            }
            return None;
        }
        if !self.armed_has(loc) {
            return None;
        }
        let slot = self.slot(loc)?;
        if !slot.armed.load(Ordering::Acquire) {
            return None;
        }
        let is_writer = slot.writers.binary_search(&tx).is_ok();
        let higher_writer = slot.writers.iter().any(|&w| w > tx);
        Some(RegionView {
            slot,
            is_writer,
            higher_writer,
        })
    }

    pub(crate) fn note_later_pass(&self, tx: TxIdx, loc: MemoryLocationHash) {
        let Some(view) = self.view(tx, loc) else {
            return;
        };
        view.note(RegionOp::Pass, 0);
    }

    /// In-frame waits already using cores. Callers park instead of spinning.
    pub(crate) fn in_frame_saturated(&self, workers: usize) -> bool {
        let cap = (workers / 2).saturating_sub(1).max(1);
        self.in_frame.load(Ordering::Relaxed) >= cap
    }

    pub(crate) fn enter_frame(&self) {
        self.in_frame.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn leave_frame(&self) {
        let prev = self.in_frame.fetch_sub(1, Ordering::Relaxed);
        if prev == 0 {
            self.in_frame.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Real write of an armed location. Counts toward Drained and wakes the edge.
    pub(crate) fn on_publish(
        &self,
        loc: MemoryLocationHash,
        writer: TxIdx,
        tips: &SfTipTable,
    ) -> Vec<TxIdx> {
        self.account(loc, writer, true, tips)
    }

    /// Predicted writer finished without storing `loc`. Wakes anyone parked on
    /// that hop so they can pick the next real writer. Does not count as data.
    pub(crate) fn on_skip(
        &self,
        loc: MemoryLocationHash,
        writer: TxIdx,
        tips: &SfTipTable,
    ) -> Vec<TxIdx> {
        self.account(loc, writer, false, tips)
    }

    fn account(
        &self,
        loc: MemoryLocationHash,
        writer: TxIdx,
        wrote: bool,
        tips: &SfTipTable,
    ) -> Vec<TxIdx> {
        if !self.enabled || !self.any_armed() || !self.armed_has(loc) {
            return Vec::new();
        }
        if let Some(slot) = self.slot(loc)
            && slot.writers.binary_search(&writer).is_ok()
        {
            let mut published = slot.published.lock().unwrap();
            let mut skipped = slot.skipped.lock().unwrap();
            if wrote {
                if !published.contains(&writer) {
                    published.push(writer);
                }
                skipped.retain(|&w| w != writer);
            } else if !published.contains(&writer) && !skipped.contains(&writer) {
                skipped.push(writer);
            }
            let accounted = published.len() + skipped.len();
            if !slot.writers.is_empty() && accounted >= slot.writers.len() {
                slot.drained.store(true, Ordering::Release);
            }
        }
        tips.wake_exact(loc, writer)
    }

    /// Radar for the next block plus the per-iteration line.
    pub(crate) fn finish(
        &self,
        writer_lists: &[(MemoryLocationHash, Vec<TxIdx>)],
        prior: &RegionRadarPrior,
    ) -> (RegionRadarPrior, String) {
        if !self.enabled {
            return (RegionRadarPrior::default(), "enabled=0".to_string());
        }
        let observed: Vec<Obs> = self
            .observed
            .lock()
            .unwrap()
            .iter()
            .map(|o| Obs {
                loc: o.loc,
                readers: o.readers.clone(),
                writers: o.writers.clone(),
                full_n: o.full_n,
                resolve_n: o.resolve_n,
            })
            .collect();
        let slots = self.slots.lock().unwrap();
        let mut acc: Vec<SeedAcc> = Vec::new();
        for seed in &prior.seeds {
            if self.excluded(seed.loc) || seed.writers.len() >= 32 {
                continue;
            }
            acc.push(SeedAcc {
                loc: seed.loc,
                writers: seed.writers.clone(),
                readers: seed.readers.clone(),
                quiet: seed.quiet,
                full_n: seed.full_hits,
                seen: false,
            });
        }
        for slot in slots.iter() {
            let waits = slot.waits.load(Ordering::Relaxed);
            let resolves = slot.resolves.load(Ordering::Relaxed);
            let fulls = slot.fulls.load(Ordering::Relaxed);
            let entry = upsert(&mut acc, slot.loc);
            entry.seen = true;
            entry.writers = merge_txs(&entry.writers, &slot.writers);
            entry.readers = merge_txs(&entry.readers, &slot.touchers);
            if waits == 0 && resolves == 0 && fulls == 0 {
                entry.quiet = entry.quiet.saturating_add(1);
            } else {
                entry.quiet = 0;
            }
            entry.full_n = entry.full_n.saturating_add(fulls as u32);
        }
        for o in &observed {
            if self.excluded(o.loc) {
                continue;
            }
            let entry = upsert(&mut acc, o.loc);
            entry.seen = true;
            entry.readers = merge_txs(&entry.readers, &o.readers);
            entry.writers = merge_txs(&entry.writers, &o.writers);
            entry.full_n = entry.full_n.saturating_add(o.full_n);
            if o.resolve_n > 0 {
                entry.quiet = 0;
            }
        }
        for (loc, writers) in writer_lists {
            if self.excluded(*loc) || writers.len() >= 32 {
                continue;
            }
            if let Some(entry) = acc.iter_mut().find(|e| e.loc == *loc) {
                entry.writers = merge_txs(&entry.writers, writers);
            }
        }
        acc.retain(|e| e.quiet < QUIET_EXIT && (e.full_n > 0 || e.seen));
        acc.sort_by(|a, b| b.full_n.cmp(&a.full_n).then(a.loc.cmp(&b.loc)));
        acc.truncate(MAX_SLOTS);
        let seeds = acc
            .into_iter()
            .map(|e| RegionSeed {
                loc: e.loc,
                writers: e.writers,
                readers: e.readers,
                quiet: e.quiet,
                full_hits: e.full_n,
            })
            .collect();
        let report = self.report_line(&slots);
        drop(slots);
        (RegionRadarPrior { seeds }, report)
    }

    fn report_line(&self, slots: &[Arc<Slot>]) -> String {
        let mut regions = String::new();
        for slot in slots {
            if !slot.armed.load(Ordering::Relaxed) && slot.resolves.load(Ordering::Relaxed) == 0 {
                continue;
            }
            let armed_at = slot.armed_at.load(Ordering::Relaxed);
            let armed_at_s = if armed_at == usize::MAX {
                "-".to_string()
            } else {
                armed_at.to_string()
            };
            if !regions.is_empty() {
                regions.push(';');
            }
            regions.push_str(&format!(
                "{:016x}@{} touch={} wait={} ord={} pass={} unstart={} retain={} full={} wait_ns={} drained={}",
                slot.loc,
                armed_at_s,
                slot.touchers.len(),
                slot.waits.load(Ordering::Relaxed),
                slot.ordered.load(Ordering::Relaxed),
                slot.passes.load(Ordering::Relaxed),
                slot.unstarted.load(Ordering::Relaxed),
                slot.retains.load(Ordering::Relaxed),
                slot.fulls.load(Ordering::Relaxed),
                slot.wait_ns.load(Ordering::Relaxed),
                u8::from(slot.drained.load(Ordering::Relaxed)),
            ));
        }
        if regions.is_empty() {
            regions.push('-');
        }
        format!(
            "enabled=1 radar={} armed={} nontoucher_probes={} full_armed={} full_unarmed={} regions={regions}",
            slots.len(),
            self.armed_n.load(Ordering::Relaxed),
            self.nontoucher_probes.load(Ordering::Relaxed),
            self.full_armed.load(Ordering::Relaxed),
            self.full_unarmed.load(Ordering::Relaxed),
        )
    }
}

struct SeedAcc {
    loc: MemoryLocationHash,
    writers: Vec<TxIdx>,
    readers: Vec<TxIdx>,
    quiet: u8,
    full_n: u32,
    seen: bool,
}

fn upsert(acc: &mut Vec<SeedAcc>, loc: MemoryLocationHash) -> &mut SeedAcc {
    if let Some(i) = acc.iter().position(|e| e.loc == loc) {
        return &mut acc[i];
    }
    acc.push(SeedAcc {
        loc,
        writers: Vec::new(),
        readers: Vec::new(),
        quiet: 0,
        full_n: 0,
        seen: false,
    });
    acc.last_mut().unwrap()
}

fn merge_txs(a: &[TxIdx], b: &[TxIdx]) -> Vec<TxIdx> {
    let mut out = a.to_vec();
    out.extend(b.iter().copied());
    out.sort_unstable();
    out.dedup();
    if out.len() > 48 {
        out.truncate(48);
    }
    out
}

fn flag_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| match std::env::var("SPECFENCE_REGION_LEARN_AVOID_V2") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(loc: u64, writers: &[TxIdx], readers: &[TxIdx]) -> RegionSeed {
        RegionSeed {
            loc,
            writers: writers.to_vec(),
            readers: readers.to_vec(),
            quiet: 0,
            full_hits: 1,
        }
    }

    #[test]
    fn non_toucher_bind_is_off_and_spine_is_not_seeded() {
        let prior = RegionRadarPrior {
            seeds: vec![seed(11, &[1, 4, 9], &[6, 8]), seed(99, &[2, 3], &[5])],
        };
        let t = RegionAvoid::from_parts(true, 16, prior, 99, EMPTY_LOC, 7);
        assert!(!t.bind(0).on, "untouched tx pays no region bind");
        assert!(t.bind(6).on && !t.bind(6).writer);
        assert!(t.bind(4).writer);
        assert_eq!(t.slots.lock().unwrap().len(), 1, "spine loc 99 stays out");
        assert_eq!(t.nontoucher_probes.load(Ordering::Relaxed), 0);
        t.view(0, 11);
        assert_eq!(
            t.nontoucher_probes.load(Ordering::Relaxed),
            0,
            "unarmed loc is not a probe"
        );
    }

    #[test]
    fn arm_flags_only_later_touchers_and_drain_on_last_writer() {
        let prior = RegionRadarPrior {
            seeds: vec![seed(11, &[1, 4, 9], &[6])],
        };
        let t = RegionAvoid::from_parts(true, 16, prior, EMPTY_LOC, EMPTY_LOC, 7);
        t.try_arm(1, 11);
        assert!(t.any_armed());
        assert_eq!(
            t.flags[1].load(Ordering::Relaxed),
            0,
            "evidence tx is not flagged"
        );
        assert_eq!(t.flags[4].load(Ordering::Relaxed), FLAG_ARMED);
        assert_eq!(t.flags[6].load(Ordering::Relaxed), FLAG_ARMED);
        assert_eq!(t.flags[9].load(Ordering::Relaxed), FLAG_ARMED);
        assert!(t.bind(0).on == false);
        let view = t.view(6, 11).expect("reader consults after arm");
        let open = |_: TxIdx| false;
        let not_done = |_: TxIdx| false;
        assert_eq!(view.pick_pred(6, open, not_done, None), Some(4));
        assert_eq!(
            view.pick_pred(6, |w| w == 4, |w| w == 4, None),
            None,
            "a real write is the floor"
        );
        assert_eq!(
            view.pick_pred(6, open, |w| w == 4, None),
            Some(1),
            "done without a write is not a floor"
        );
        assert_eq!(
            view.pick_pred(6, |w| w == 1, |w| w == 1, Some(5)),
            Some(5),
            "live writer above the floor is the hop"
        );
        assert_eq!(
            view.pick_pred(6, |w| w == 4, |w| w == 4, Some(2)),
            None,
            "live writer behind the floor is ignored"
        );
        assert_eq!(
            view.decide(PredState::Live),
            RegionOp::Wait { ordered: false }
        );
        assert_eq!(view.decide(PredState::Unstarted), RegionOp::Unstarted);
        assert_eq!(view.decide(PredState::Published), RegionOp::Retain);
        let w = t.view(9, 11).expect("later writer");
        assert_eq!(w.pick_pred(9, open, not_done, None), Some(4));
        assert_eq!(w.decide(PredState::Live), RegionOp::Wait { ordered: true });
        let tips = SfTipTable::new();
        t.on_publish(11, 1, &tips);
        t.on_skip(11, 4, &tips);
        assert!(
            !t.slot(11).unwrap().drained.load(Ordering::Relaxed),
            "a skip is not a publish"
        );
        t.on_publish(11, 9, &tips);
        assert!(t.slot(11).unwrap().drained.load(Ordering::Relaxed));
        let after = t.view(6, 11).unwrap();
        assert_eq!(after.decide(PredState::Live), RegionOp::Pass);
    }

    #[test]
    fn disabled_table_does_not_arm() {
        let t = RegionAvoid::disabled();
        assert!(!t.enabled());
        assert!(!t.bind(1).on);
        t.try_arm(1, 11);
        t.note_raw_evidence(4, 11, 1);
        t.on_resolve(11, 4, true, Some(1));
        assert!(!t.any_armed());
        assert!(t.view(4, 11).is_none());
        let (next, line) = t.finish(&[], &RegionRadarPrior::default());
        assert!(next.seeds.is_empty());
        assert_eq!(line, "enabled=0");
    }
}
