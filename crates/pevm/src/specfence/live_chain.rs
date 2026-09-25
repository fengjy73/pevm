//! In-block ordered writer chains.
//!
//! A writer is inserted when it is discovered: a published write, a running
//! write intent, abort residual, or a same-class prediction. Cross-block
//! priors occupy radar bits only and never bind a wait.
//!
//! Chain budget and the wait threshold move inside safety bounds from the
//! block's abort rate, idle rate, and per-class hit rate.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};

use hashbrown::HashMap;
use rustc_hash::FxBuildHasher;
use smallvec::SmallVec;

use crate::TxIdx;

/// Safety bounds. The live values start inside these and move during the block.
const K_LIMIT: usize = 256;
const K_FLOOR: usize = 8;
const P_FLOOR: f64 = 0.05;
const P_CEIL: f64 = 0.95;
const CLASS_HIT_FLOOR: f64 = 0.15;
const C_ABORT_MIN: u64 = 5_000;
const C_ABORT_MAX: u64 = 2_000_000;

const ST_EMPTY: u64 = 0;
const ST_PREDICTED: u64 = 1;
const ST_RUNNING: u64 = 2;
const ST_PUBLISHED: u64 = 3;

/// How transactions are grouped before a location is predicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKeyKind {
    /// `(to, selector)`.
    ToSelector,
    /// `(code_hash, selector)`.
    CodeHashSelector,
}

impl ClassKeyKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ToSelector => "to+selector",
            Self::CodeHashSelector => "code_hash+selector",
        }
    }
}

#[derive(Clone)]
pub(crate) struct ClassGroup {
    pub(crate) members: Vec<TxIdx>,
    /// Calldata carried a selector. Plain value transfers stay out of the
    /// class-head barrier; their shared recipient is preseeded instead.
    pub(crate) contract: bool,
}

struct Chain {
    member: Vec<AtomicU64>,
    radar: Vec<AtomicU64>,
    /// Packed state per tx: low 2 bits are the state, next 16 the incarnation.
    state: Vec<AtomicU64>,
    armed: AtomicBool,
    /// Class spines and abort locations stay. Eviction must not drop them.
    pinned: AtomicBool,
    writers: AtomicUsize,
    /// Fixed-point hit/miss used as a chain credit (wait helped vs wait failed).
    ok: AtomicU64,
    fail: AtomicU64,
    class_seeded: AtomicBool,
    live: AtomicBool,
}

impl Chain {
    fn new(n: usize) -> Self {
        let words = n.div_ceil(64).max(1);
        Self {
            member: (0..words).map(|_| AtomicU64::new(0)).collect(),
            radar: (0..words).map(|_| AtomicU64::new(0)).collect(),
            state: (0..n).map(|_| AtomicU64::new(0)).collect(),
            armed: AtomicBool::new(false),
            pinned: AtomicBool::new(false),
            writers: AtomicUsize::new(0),
            ok: AtomicU64::new(1),
            fail: AtomicU64::new(1),
            class_seeded: AtomicBool::new(false),
            live: AtomicBool::new(false),
        }
    }

    fn reset_slot(&self) {
        for w in &self.member {
            w.store(0, Ordering::Relaxed);
        }
        for w in &self.radar {
            w.store(0, Ordering::Relaxed);
        }
        for s in &self.state {
            s.store(0, Ordering::Relaxed);
        }
        self.armed.store(false, Ordering::Relaxed);
        self.pinned.store(false, Ordering::Relaxed);
        self.writers.store(0, Ordering::Relaxed);
        self.ok.store(1, Ordering::Relaxed);
        self.fail.store(1, Ordering::Relaxed);
        self.class_seeded.store(false, Ordering::Relaxed);
        self.live.store(false, Ordering::Relaxed);
    }

    fn set_member(&self, tx: TxIdx) {
        let word = tx / 64;
        let bit = 1u64 << (tx % 64);
        if word < self.member.len() {
            self.member[word].fetch_or(bit, Ordering::Release);
        }
    }

    fn clear_member(&self, tx: TxIdx) {
        let word = tx / 64;
        let bit = 1u64 << (tx % 64);
        if word < self.member.len() {
            self.member[word].fetch_and(!bit, Ordering::Release);
        }
    }

    fn nearest_lower(&self, tx: TxIdx) -> Option<TxIdx> {
        if tx == 0 {
            return None;
        }
        let mut i = tx - 1;
        loop {
            let word = i / 64;
            let bit = i % 64;
            let mask = if bit == 63 {
                u64::MAX
            } else {
                (1u64 << (bit + 1)) - 1
            };
            let bits = self.member[word].load(Ordering::Acquire) & mask;
            if bits != 0 {
                let highest = 63 - bits.leading_zeros() as usize;
                return Some(word * 64 + highest);
            }
            if word == 0 {
                return None;
            }
            i = word * 64 - 1;
        }
    }

    const fn pack(state: u64, incarnation: usize) -> u64 {
        state | ((incarnation as u64 & 0xffff) << 2)
    }

    const fn unpack(raw: u64) -> (u64, usize) {
        (raw & 0b11, ((raw >> 2) & 0xffff) as usize)
    }

    fn popcount(&self) -> usize {
        self.member
            .iter()
            .map(|w| w.load(Ordering::Relaxed).count_ones() as usize)
            .sum()
    }
}

struct Directory {
    /// Location hash → chain slot.
    index: HashMap<u64, usize, FxBuildHasher>,
}

#[derive(Clone, Copy)]
struct SlotRef {
    slot: u16,
    /// Read-then-write. Blind and lazy touches stay off this bit.
    admit: bool,
}

/// First writer of a location that does not yet own a chain slot.
struct Sighting {
    tx: TxIdx,
    incarnation: usize,
    /// Read-then-write. Blind and lazy writes stay off the admission list.
    rmw: bool,
}

pub(crate) struct LiveChain {
    n: usize,
    chains: Box<[Chain]>,
    dir: RwLock<Directory>,
    used: AtomicUsize,
    k_max: AtomicUsize,
    any: AtomicBool,
    class_of_tx: Vec<u16>,
    /// `Basic(to)` hash per transaction. `0` is a create or an unknown target.
    to_of: Vec<u64>,
    classes: Vec<ClassGroup>,
    class_hit: Vec<AtomicU64>,
    class_miss: Vec<AtomicU64>,
    class_open: Vec<AtomicBool>,
    /// Same-class head and sibling prediction stay off until this block records
    /// a validation failure or abort for that class.
    class_barrier: Vec<AtomicBool>,
    membership: Vec<Mutex<SmallVec<[SlotRef; 4]>>>,
    /// Abort cost estimate in nanoseconds. Moves inside [`C_ABORT_MIN`, `C_ABORT_MAX`].
    c_abort_ns: AtomicU64,
    /// Multiplier on the wait threshold. Grows when workers are idle.
    wait_scale_q8: AtomicU64,
    finished: AtomicUsize,
    aborted: AtomicUsize,
    idle_polls: AtomicUsize,
    busy_polls: AtomicUsize,
    /// Locations seen once. The second writer allocates a chain and backfills both.
    seen: Mutex<HashMap<u64, Sighting, FxBuildHasher>>,
    /// Beneficiary basic-account hash. Every transaction writes it lazily;
    /// chaining it would report a chain of length `n` and hide real spines.
    skip: std::sync::atomic::AtomicU64,
    skip_on: AtomicBool,
    /// One worker already runs and commits in index order. The chain would
    /// only add directory locks and allocations.
    serial: bool,
}

impl LiveChain {
    pub(crate) fn new(n: usize, _kind: ClassKeyKind) -> Self {
        let start_k = (n / 32).clamp(K_FLOOR, 64);
        Self {
            n,
            chains: (0..K_LIMIT)
                .map(|_| Chain::new(n))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            dir: RwLock::new(Directory {
                index: HashMap::with_hasher(FxBuildHasher),
            }),
            used: AtomicUsize::new(0),
            k_max: AtomicUsize::new(start_k),
            any: AtomicBool::new(false),
            class_of_tx: vec![u16::MAX; n],
            to_of: vec![0; n],
            classes: Vec::new(),
            class_hit: Vec::new(),
            class_miss: Vec::new(),
            class_open: Vec::new(),
            class_barrier: Vec::new(),
            membership: (0..n).map(|_| Mutex::new(SmallVec::new())).collect(),
            c_abort_ns: AtomicU64::new(50_000),
            wait_scale_q8: AtomicU64::new(256),
            finished: AtomicUsize::new(0),
            aborted: AtomicUsize::new(0),
            idle_polls: AtomicUsize::new(0),
            busy_polls: AtomicUsize::new(0),
            seen: Mutex::new(HashMap::with_hasher(FxBuildHasher)),
            skip: std::sync::atomic::AtomicU64::new(0),
            skip_on: AtomicBool::new(false),
            serial: false,
        }
    }

    /// No chains and no per-transaction membership. Used when `workers == 1`.
    pub(crate) fn untracked(n: usize) -> Self {
        Self {
            n,
            chains: Box::new([]),
            dir: RwLock::new(Directory {
                index: HashMap::with_hasher(FxBuildHasher),
            }),
            used: AtomicUsize::new(0),
            k_max: AtomicUsize::new(0),
            any: AtomicBool::new(false),
            class_of_tx: Vec::new(),
            to_of: Vec::new(),
            classes: Vec::new(),
            class_hit: Vec::new(),
            class_miss: Vec::new(),
            class_open: Vec::new(),
            class_barrier: Vec::new(),
            membership: Vec::new(),
            c_abort_ns: AtomicU64::new(50_000),
            wait_scale_q8: AtomicU64::new(256),
            finished: AtomicUsize::new(0),
            aborted: AtomicUsize::new(0),
            idle_polls: AtomicUsize::new(0),
            busy_polls: AtomicUsize::new(0),
            seen: Mutex::new(HashMap::with_hasher(FxBuildHasher)),
            skip: std::sync::atomic::AtomicU64::new(0),
            skip_on: AtomicBool::new(false),
            serial: true,
        }
    }

    /// Ignore this location in the ordered chains. Used for the beneficiary.
    pub(crate) fn skip_location(&self, location: u64) {
        self.skip.store(location, Ordering::Relaxed);
        self.skip_on.store(true, Ordering::Relaxed);
    }

    pub(crate) fn install_classes(
        &mut self,
        classes: Vec<ClassGroup>,
        class_of_tx: Vec<u16>,
        to_of: Vec<u64>,
    ) {
        let n = classes.len();
        self.class_hit = (0..n).map(|_| AtomicU64::new(1)).collect();
        self.class_miss = (0..n).map(|_| AtomicU64::new(1)).collect();
        self.class_open = (0..n).map(|_| AtomicBool::new(true)).collect();
        self.class_barrier = (0..n).map(|_| AtomicBool::new(false)).collect();
        self.classes = classes;
        self.class_of_tx = class_of_tx;
        self.to_of = to_of;
    }

    /// Before any worker runs, every repeated `Basic(to)` is an armed chain.
    ///
    /// Members are `Predicted` and are not admission edges. A later
    /// read-then-write can set the admit bit. Blind and lazy publishes do not.
    pub(crate) fn preseed_recipients(&self) {
        let mut groups: HashMap<u64, Vec<TxIdx>, FxBuildHasher> =
            HashMap::with_hasher(FxBuildHasher);
        for (tx, &location) in self.to_of.iter().enumerate() {
            if location == 0 {
                continue;
            }
            if self.skip_on.load(Ordering::Relaxed) && self.skip.load(Ordering::Relaxed) == location
            {
                continue;
            }
            groups.entry(location).or_default().push(tx);
        }
        let mut groups: Vec<_> = groups
            .into_iter()
            .filter(|(_, members)| members.len() >= 2)
            .collect();
        groups.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));
        if groups.is_empty() {
            return;
        }
        let room = (groups.len() + 32).clamp(K_FLOOR, K_LIMIT);
        let k = self.k_max.load(Ordering::Relaxed);
        if room > k {
            self.k_max.store(room, Ordering::Relaxed);
        }
        for (location, members) in groups {
            self.preseed_one(location, &members);
        }
    }

    fn preseed_one(&self, location: u64, txs: &[TxIdx]) {
        let Some(idx) = self.ensure_slot(location, txs.len().clamp(2, K_LIMIT), true) else {
            return;
        };
        let chain = &self.chains[idx];
        chain.pinned.store(true, Ordering::Relaxed);
        chain.class_seeded.store(true, Ordering::Relaxed);
        chain.armed.store(true, Ordering::Release);
        for &tx in txs {
            if tx >= self.n {
                continue;
            }
            let (state, _) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
            if state == ST_EMPTY {
                chain.writers.fetch_add(1, Ordering::Relaxed);
            }
            chain.set_member(tx);
            chain.state[tx].store(Chain::pack(ST_PREDICTED, 0), Ordering::Release);
            self.remember(tx, idx, false);
        }
    }

    /// Radar only. Does not set member bits and does not arm a wait.
    pub(crate) fn install_radar(&self, location: u64, txs: &[TxIdx]) {
        let Some(idx) = self.ensure_slot(location, 0, false) else {
            return;
        };
        let chain = &self.chains[idx];
        for &tx in txs {
            if tx >= self.n {
                continue;
            }
            let word = tx / 64;
            let bit = 1u64 << (tx % 64);
            if word < chain.radar.len() {
                chain.radar[word].fetch_or(bit, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn any(&self) -> bool {
        self.any.load(Ordering::Relaxed)
    }

    fn class_allows(&self, class: u16) -> bool {
        let i = class as usize;
        if !self
            .class_open
            .get(i)
            .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            return false;
        }
        let hit = self.class_hit[i].load(Ordering::Relaxed) as f64;
        let miss = self.class_miss[i].load(Ordering::Relaxed) as f64;
        hit / (hit + miss) >= CLASS_HIT_FLOOR
    }

    fn remember(&self, tx: TxIdx, slot: usize, admit: bool) {
        let mut mem = self.membership[tx].lock().unwrap();
        let slot = slot as u16;
        if let Some(found) = mem.iter_mut().find(|entry| entry.slot == slot) {
            found.admit |= admit;
        } else {
            mem.push(SlotRef { slot, admit });
        }
    }

    fn ensure_slot(&self, location: u64, priority: usize, pin: bool) -> Option<usize> {
        {
            let dir = self.dir.read().unwrap();
            if let Some(&idx) = dir.index.get(&location) {
                return Some(idx);
            }
        }
        let mut dir = self.dir.write().unwrap();
        if let Some(&idx) = dir.index.get(&location) {
            if pin {
                self.chains[idx].pinned.store(true, Ordering::Relaxed);
            }
            return Some(idx);
        }
        let k_max = self.k_max.load(Ordering::Relaxed).min(K_LIMIT);
        let used = self.used.load(Ordering::Relaxed);
        if used < k_max {
            let idx = used;
            self.chains[idx].reset_slot();
            self.chains[idx].live.store(true, Ordering::Release);
            if pin {
                self.chains[idx].pinned.store(true, Ordering::Relaxed);
            }
            self.used.store(used + 1, Ordering::Relaxed);
            dir.index.insert(location, idx);
            self.any.store(true, Ordering::Release);
            return Some(idx);
        }
        // Replace the lowest-credit chain when the new location is hotter.
        let mut worst: Option<(usize, u64)> = None;
        for (idx, chain) in self.chains.iter().enumerate().take(used) {
            if !chain.live.load(Ordering::Relaxed) || chain.pinned.load(Ordering::Relaxed) {
                continue;
            }
            let credit =
                chain.writers.load(Ordering::Relaxed) as u64 + chain.ok.load(Ordering::Relaxed);
            let fail = chain.fail.load(Ordering::Relaxed);
            // Failures raise the score. Those locations are why the chain exists.
            let score = credit
                .saturating_mul(4)
                .saturating_add(fail.saturating_mul(32));
            if worst.is_none_or(|(_, s)| score < s) {
                worst = Some((idx, score));
            }
        }
        let new_score = priority as u64;
        if let Some((idx, score)) = worst
            && new_score > score
        {
            self.detach(idx);
            self.chains[idx].reset_slot();
            self.chains[idx].live.store(true, Ordering::Release);
            if pin {
                self.chains[idx].pinned.store(true, Ordering::Relaxed);
            }
            dir.index.retain(|_, v| *v != idx);
            dir.index.insert(location, idx);
            self.any.store(true, Ordering::Release);
            return Some(idx);
        }
        None
    }

    fn slot_of(&self, location: u64) -> Option<usize> {
        if !self.any.load(Ordering::Relaxed) {
            return None;
        }
        self.dir.read().unwrap().index.get(&location).copied()
    }

    pub(crate) fn nearest_lower(&self, location: u64, tx: TxIdx) -> Option<TxIdx> {
        let idx = self.slot_of(location)?;
        self.chains[idx].nearest_lower(tx)
    }

    pub(crate) fn is_armed(&self, location: u64) -> bool {
        self.slot_of(location)
            .is_some_and(|idx| self.chains[idx].armed.load(Ordering::Relaxed))
    }

    pub(crate) fn writer_state(&self, location: u64, writer: TxIdx) -> (u64, usize) {
        let Some(idx) = self.slot_of(location) else {
            return (ST_EMPTY, 0);
        };
        if writer >= self.n {
            return (ST_EMPTY, 0);
        }
        Chain::unpack(self.chains[idx].state[writer].load(Ordering::Acquire))
    }

    pub(crate) fn writer_published(&self, location: u64, writer: TxIdx) -> bool {
        self.writer_state(location, writer).0 == ST_PUBLISHED
    }

    /// A finished predicted or running writer that never published is a hole.
    /// Drop it so the next lower writer becomes visible. Published writers stay.
    pub(crate) fn clear_hole(&self, location: u64, tx: TxIdx) {
        let Some(idx) = self.slot_of(location) else {
            return;
        };
        if tx >= self.n {
            return;
        }
        let chain = &self.chains[idx];
        let (state, _) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
        if state == ST_PUBLISHED || state == ST_EMPTY {
            return;
        }
        chain.clear_member(tx);
        chain.state[tx].store(ST_EMPTY, Ordering::Release);
        if chain.writers.load(Ordering::Relaxed) > 0 {
            chain.writers.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn member_bit(&self, location: u64, tx: TxIdx) -> bool {
        let Some(idx) = self.slot_of(location) else {
            return false;
        };
        if tx >= self.n {
            return false;
        }
        let word = tx / 64;
        let bit = 1u64 << (tx % 64);
        self.chains[idx]
            .member
            .get(word)
            .is_some_and(|w| w.load(Ordering::Acquire) & bit != 0)
    }

    /// Predicted and running writers wait when the abort cost still beats the wait.
    pub(crate) fn should_wait(&self, location: u64, writer: TxIdx, writer_executing: bool) -> bool {
        let (state, _) = self.writer_state(location, writer);
        let p = match state {
            ST_RUNNING => 0.95,
            ST_PREDICTED => {
                let class = self.class_of_tx.get(writer).copied().unwrap_or(u16::MAX);
                if class == u16::MAX {
                    0.5
                } else {
                    let hit = self.class_hit[class as usize].load(Ordering::Relaxed) as f64;
                    let miss = self.class_miss[class as usize].load(Ordering::Relaxed) as f64;
                    (hit / (hit + miss)).clamp(P_FLOOR, P_CEIL)
                }
            }
            _ => return false,
        };
        if p < P_FLOOR {
            return false;
        }
        let c_abort = self.c_abort_ns.load(Ordering::Relaxed) as f64;
        let scale = self.wait_scale_q8.load(Ordering::Relaxed) as f64 / 256.0;
        let e_wait = if writer_executing { 8_000.0 } else { 20_000.0 };
        p * c_abort > e_wait * scale
    }

    /// Predecessor this tx should wait for before it is admitted, if any.
    pub(crate) fn admission_predecessor(&self, tx: TxIdx) -> Option<TxIdx> {
        if self.serial || !self.any.load(Ordering::Relaxed) || tx == 0 {
            return None;
        }
        let mem = self.membership[tx].lock().unwrap();
        let mut best: Option<TxIdx> = None;
        for entry in mem.iter().filter(|entry| entry.admit) {
            let chain = &self.chains[entry.slot as usize];
            let Some(pred) = chain.nearest_lower(tx) else {
                continue;
            };
            let (state, _) = Chain::unpack(chain.state[pred].load(Ordering::Acquire));
            if state == ST_PUBLISHED || state == ST_EMPTY {
                continue;
            }
            // Location hash is unknown here; the cost model only needs the state.
            let p = if state == ST_RUNNING { 0.95 } else { 0.5 };
            let c_abort = self.c_abort_ns.load(Ordering::Relaxed) as f64;
            let scale = self.wait_scale_q8.load(Ordering::Relaxed) as f64 / 256.0;
            if p * c_abort <= 20_000.0 * scale {
                continue;
            }
            best = Some(best.map_or(pred, |b| b.max(pred)));
        }
        best
    }

    pub(crate) fn mark_running(&self, tx: TxIdx, incarnation: usize) {
        if self.serial {
            return;
        }
        let mem = self.membership[tx].lock().unwrap().clone();
        for entry in mem {
            let chain = &self.chains[entry.slot as usize];
            let (state, _) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
            if state == ST_PREDICTED || state == ST_RUNNING {
                chain.state[tx].store(Chain::pack(ST_RUNNING, incarnation), Ordering::Release);
            }
        }
    }

    /// Lowest-index classmate, when this transaction must not start until that
    /// head has finished. The head publishes read-then-write locations and
    /// predicts the rest of the class before those transactions read.
    ///
    /// Plain transfers return `None`. Their shared recipient is already a
    /// preseeded chain, and a head barrier would hold that chain's predicted
    /// writers behind one transaction.
    pub(crate) fn class_id(&self, tx: TxIdx) -> u16 {
        self.class_of_tx.get(tx).copied().unwrap_or(u16::MAX)
    }

    pub(crate) fn class_head(&self, tx: TxIdx) -> Option<TxIdx> {
        if self.serial {
            return None;
        }
        let class = self.class_of_tx.get(tx).copied().unwrap_or(u16::MAX);
        if class == u16::MAX {
            return None;
        }
        let group = self.classes.get(class as usize)?;
        if !group.contract {
            return None;
        }
        // Off until this block has a conflict on the class. Classmates that
        // do not share a location are not serialized by default.
        if !self
            .class_barrier
            .get(class as usize)
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return None;
        }
        let head = *group.members.first()?;
        if head >= tx { None } else { Some(head) }
    }

    /// Turn the class-head barrier and same-class sibling prediction on.
    pub(crate) fn note_class_conflict(&self, class: u16) {
        if class == u16::MAX {
            return;
        }
        if let Some(flag) = self.class_barrier.get(class as usize) {
            flag.store(true, Ordering::Release);
        }
    }

    /// `true` when a member in `[start, end)` is not yet a final write or a final hole.
    ///
    /// `start` is one past the read-from origin, or zero when the read saw
    /// storage. Members below the origin are that writer's dependency, not
    /// this reader's. `resolved(tx, chain_incarnation, published)` is the test.
    pub(crate) fn any_unresolved_between(
        &self,
        location: u64,
        start: TxIdx,
        end: TxIdx,
        mut resolved: impl FnMut(TxIdx, usize, bool) -> bool,
    ) -> bool {
        if self.serial || start >= end {
            return false;
        }
        let Some(idx) = self.slot_of(location) else {
            return false;
        };
        let chain = &self.chains[idx];
        if !chain.armed.load(Ordering::Relaxed) {
            return false;
        }
        let last = end.min(self.n);
        let mut tx = start;
        while tx < last {
            let word = tx / 64;
            let bits = chain
                .member
                .get(word)
                .map(|w| w.load(Ordering::Acquire))
                .unwrap_or(0);
            if bits == 0 {
                tx = (word + 1) * 64;
                continue;
            }
            let base = word * 64;
            let mut bit = tx - base;
            while bit < 64 && base + bit < last {
                if bits & (1u64 << bit) != 0 {
                    let member = base + bit;
                    let (state, inc) = Chain::unpack(chain.state[member].load(Ordering::Acquire));
                    if state != ST_EMPTY && !resolved(member, inc, state == ST_PUBLISHED) {
                        return true;
                    }
                }
                bit += 1;
            }
            tx = (word + 1) * 64;
        }
        false
    }

    /// Locations this transaction is still a chain member of.
    pub(crate) fn member_locations(&self, tx: TxIdx) -> Vec<u64> {
        if self.serial || tx >= self.membership.len() {
            return Vec::new();
        }
        let slots: Vec<u16> = self.membership[tx]
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.slot)
            .collect();
        if slots.is_empty() {
            return Vec::new();
        }
        let dir = self.dir.read().unwrap();
        dir.index
            .iter()
            .filter(|(_, slot)| slots.contains(&(**slot as u16)))
            .map(|(location, _)| *location)
            .collect()
    }

    /// Publish one write into the ordered chain.
    ///
    /// Read-then-write joins the admission list so the writer waits for the
    /// previous final value. Blind and lazy writes are visible to readers
    /// (member bit, `Published`) and do not make writers wait on each other.
    /// The publisher itself is inserted on the first write when a slot is
    /// free. Same-class prediction runs only for a read-then-write: a lazy
    /// sender write must not mark every classmate as a writer of that account.
    pub(crate) fn publish_write(
        &self,
        tx: TxIdx,
        incarnation: usize,
        location: u64,
        rmw: bool,
        tx_open: impl Fn(TxIdx) -> bool,
    ) {
        if self.serial {
            return;
        }
        if self.skip_on.load(Ordering::Relaxed) && self.skip.load(Ordering::Relaxed) == location {
            return;
        }
        if let Some(idx) = self.slot_of(location) {
            self.mark_published(idx, tx, incarnation, rmw);
            return;
        }
        let class = self.class_of_tx.get(tx).copied().unwrap_or(u16::MAX);
        let class_len = if class == u16::MAX {
            1
        } else {
            self.classes[class as usize].members.len()
        };
        // Hold `seen` across slot allocation so a peer cannot record a
        // sighting that misses the new chain.
        let mut seen = self.seen.lock().unwrap();
        if let Some(idx) = self.slot_of(location) {
            drop(seen);
            self.mark_published(idx, tx, incarnation, rmw);
            return;
        }
        // Same-class prediction is a read-then-write edge. Blind and lazy
        // writes insert only this publisher.
        let open_for_class = rmw && class_len >= 2 && self.class_allows(class);
        let prev = seen.remove(&location);
        if prev.is_none() && !open_for_class {
            drop(seen);
            // First publisher is visible immediately when a slot is free.
            // Readers then wait for that write instead of for a second publisher.
            if let Some(idx) = self.ensure_slot(location, 1, false) {
                self.mark_published(idx, tx, incarnation, rmw);
                return;
            }
            let mut seen = self.seen.lock().unwrap();
            if let Some(idx) = self.slot_of(location) {
                drop(seen);
                self.mark_published(idx, tx, incarnation, rmw);
                return;
            }
            seen.insert(
                location,
                Sighting {
                    tx,
                    incarnation,
                    rmw,
                },
            );
            return;
        }
        // A plain-transfer class can be hundreds of transactions. That must
        // not outrank a location that has already failed validation.
        let priority = if open_for_class {
            class_len.clamp(2, 32)
        } else {
            2
        };
        let Some(idx) = self.ensure_slot(location, priority, open_for_class) else {
            seen.insert(
                location,
                prev.unwrap_or(Sighting {
                    tx,
                    incarnation,
                    rmw,
                }),
            );
            return;
        };
        drop(seen);
        if let Some(prev) = prev {
            self.mark_published(idx, prev.tx, prev.incarnation, prev.rmw);
        }
        self.mark_published(idx, tx, incarnation, rmw);
        if open_for_class {
            self.chains[idx].pinned.store(true, Ordering::Relaxed);
            self.predict_siblings(idx, tx, class, &tx_open, rmw);
        }
    }

    fn mark_published(&self, idx: usize, tx: TxIdx, incarnation: usize, admit: bool) {
        if tx >= self.n {
            return;
        }
        let chain = &self.chains[idx];
        let prev = Chain::unpack(chain.state[tx].load(Ordering::Acquire)).0;
        if prev == ST_EMPTY {
            chain.writers.fetch_add(1, Ordering::Relaxed);
        }
        chain.set_member(tx);
        chain.state[tx].store(Chain::pack(ST_PUBLISHED, incarnation), Ordering::Release);
        if admit {
            self.remember(tx, idx, true);
        } else {
            self.remember(tx, idx, false);
        }
        let writers = chain.writers.load(Ordering::Relaxed);
        if writers >= 2 || chain.class_seeded.load(Ordering::Relaxed) {
            chain.pinned.store(true, Ordering::Relaxed);
            chain.armed.store(true, Ordering::Release);
        } else if writers >= 1 {
            // One known writer is already a RAW edge for later readers.
            chain.armed.store(true, Ordering::Release);
        }
    }

    fn predict_siblings(
        &self,
        idx: usize,
        tx: TxIdx,
        class: u16,
        tx_open: &impl Fn(TxIdx) -> bool,
        admit: bool,
    ) {
        if class == u16::MAX {
            return;
        }
        // No conflict in this class yet. The publisher is already on the chain;
        // classmates are not guessed to be writers of this location.
        if !self
            .class_barrier
            .get(class as usize)
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return;
        }
        let chain = &self.chains[idx];
        if chain.class_seeded.swap(true, Ordering::AcqRel) {
            return;
        }
        chain.pinned.store(true, Ordering::Relaxed);
        chain.armed.store(true, Ordering::Release);
        self.insert_same_target(idx, tx, class, tx_open, admit);
    }

    /// Add classmates that are not on the chain yet.
    ///
    /// Safe to call after the chain is already seeded. Blind and lazy callers
    /// pass `admit = false`.
    pub(crate) fn insert_same_target(
        &self,
        idx: usize,
        tx: TxIdx,
        class: u16,
        tx_open: &impl Fn(TxIdx) -> bool,
        admit: bool,
    ) {
        if class == u16::MAX || idx >= self.chains.len() {
            return;
        }
        let chain = &self.chains[idx];
        chain.armed.store(true, Ordering::Release);
        for &member in &self.classes[class as usize].members {
            if member == tx || member >= self.n || !tx_open(member) {
                continue;
            }
            let (state, _) = Chain::unpack(chain.state[member].load(Ordering::Acquire));
            if state == ST_EMPTY {
                chain.set_member(member);
                chain.state[member].store(Chain::pack(ST_PREDICTED, 0), Ordering::Release);
                chain.writers.fetch_add(1, Ordering::Relaxed);
                self.remember(member, idx, admit);
            }
        }
    }

    /// Abort evidence: the invalidating writer and its same-target classmates
    /// join the chain even if they have not published yet.
    pub(crate) fn note_conflict_writer(
        &self,
        location: u64,
        writer: TxIdx,
        tx_open: impl Fn(TxIdx) -> bool,
    ) {
        let Some(idx) = self.slot_of(location) else {
            return;
        };
        let class = self.class_of_tx.get(writer).copied().unwrap_or(u16::MAX);
        self.insert_same_target(idx, writer, class, &tx_open, false);
    }

    /// Drop admission membership for a slot that is about to be reused.
    fn detach(&self, idx: usize) {
        let chain = &self.chains[idx];
        for (word_i, word) in chain.member.iter().enumerate() {
            let bits = word.load(Ordering::Relaxed);
            if bits == 0 {
                continue;
            }
            for bit in 0..64 {
                if bits & (1u64 << bit) == 0 {
                    continue;
                }
                let tx = word_i * 64 + bit;
                if tx >= self.n {
                    break;
                }
                self.membership[tx]
                    .lock()
                    .unwrap()
                    .retain(|entry| entry.slot != idx as u16);
            }
        }
    }

    /// Validation failed on `location`. Backfill is the caller's mv scan.
    pub(crate) fn arm_failure(&self, location: u64) {
        if let Some(idx) = self.ensure_slot(location, K_LIMIT, true) {
            let chain = &self.chains[idx];
            chain.pinned.store(true, Ordering::Relaxed);
            chain.armed.store(true, Ordering::Release);
            chain.fail.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn note_backfill_writer(&self, location: u64, tx: TxIdx, incarnation: usize) {
        let Some(idx) = self.slot_of(location) else {
            return;
        };
        let chain = &self.chains[idx];
        let (state, _) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
        if state == ST_EMPTY {
            chain.set_member(tx);
            chain.writers.fetch_add(1, Ordering::Relaxed);
        }
        if state != ST_PUBLISHED {
            chain.state[tx].store(Chain::pack(ST_PUBLISHED, incarnation), Ordering::Release);
        }
    }

    /// Previous incarnation's write set becomes a high-confidence prediction.
    pub(crate) fn on_abort_residual(&self, tx: TxIdx, incarnation: usize, locations: &[u64]) {
        for &location in locations {
            let Some(idx) = self.ensure_slot(location, 8, true) else {
                continue;
            };
            let chain = &self.chains[idx];
            chain.pinned.store(true, Ordering::Relaxed);
            chain.set_member(tx);
            chain.state[tx].store(Chain::pack(ST_PREDICTED, incarnation), Ordering::Release);
            chain.armed.store(true, Ordering::Release);
            // Keep an admit bit that a read-then-write already set.
            self.remember(tx, idx, false);
        }
    }

    /// Drop predicted bits the committed tx did not actually write, and wake is external.
    pub(crate) fn hole_clear(&self, tx: TxIdx, written: &[u64]) {
        if self.serial {
            return;
        }
        let mem = self.membership[tx].lock().unwrap().clone();
        for entry in mem {
            let chain = &self.chains[entry.slot as usize];
            let (state, inc) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
            if state == ST_PUBLISHED {
                let _ = (written, inc);
                continue;
            }
            chain.clear_member(tx);
            chain.state[tx].store(ST_EMPTY, Ordering::Release);
            if state != ST_EMPTY {
                chain.writers.fetch_sub(1, Ordering::Relaxed);
                if let Some(&class) = self.class_of_tx.get(tx)
                    && class != u16::MAX
                {
                    self.class_miss[class as usize].fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if let Some(&class) = self.class_of_tx.get(tx)
            && class != u16::MAX
            && written.iter().any(|loc| self.slot_of(*loc).is_some())
        {
            self.class_hit[class as usize].fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn note_finished(&self, aborted: bool) {
        self.finished.fetch_add(1, Ordering::Relaxed);
        if aborted {
            self.aborted.fetch_add(1, Ordering::Relaxed);
        }
        let finished = self.finished.load(Ordering::Relaxed);
        if finished.is_multiple_of(32) {
            self.tick();
        }
    }

    pub(crate) fn note_idle(&self, idle: bool) {
        if idle {
            self.idle_polls.fetch_add(1, Ordering::Relaxed);
        } else {
            self.busy_polls.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn tick(&self) {
        let finished = self.finished.load(Ordering::Relaxed).max(1) as f64;
        let abort_rate = self.aborted.load(Ordering::Relaxed) as f64 / finished;
        let idle = self.idle_polls.load(Ordering::Relaxed) as f64;
        let busy = self.busy_polls.load(Ordering::Relaxed).max(1) as f64;
        let idle_rate = idle / (idle + busy);
        let mut k = self.k_max.load(Ordering::Relaxed);
        if abort_rate > 0.15 {
            k = (k + 8).min(K_LIMIT);
            let c = self.c_abort_ns.load(Ordering::Relaxed);
            self.c_abort_ns.store(
                (c.saturating_mul(5) / 4).clamp(C_ABORT_MIN, C_ABORT_MAX),
                Ordering::Relaxed,
            );
        } else if abort_rate < 0.02 && k > K_FLOOR {
            let used = self.used.load(Ordering::Relaxed);
            let pinned = self
                .chains
                .iter()
                .take(used)
                .filter(|chain| chain.pinned.load(Ordering::Relaxed))
                .count();
            if k > pinned.saturating_add(8) {
                k -= 1;
            }
        }
        self.k_max.store(k, Ordering::Relaxed);
        let mut scale = self.wait_scale_q8.load(Ordering::Relaxed);
        if idle_rate > 0.5 {
            scale = (scale + 32).min(256 * 8);
        } else if idle_rate < 0.05 {
            scale = scale.saturating_sub(16).max(64);
        }
        self.wait_scale_q8.store(scale, Ordering::Relaxed);
        for (i, open) in self.class_open.iter().enumerate() {
            let hit = self.class_hit[i].load(Ordering::Relaxed) as f64;
            let miss = self.class_miss[i].load(Ordering::Relaxed) as f64;
            if hit + miss > 4.0 && hit / (hit + miss) < CLASS_HIT_FLOOR {
                open.store(false, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn max_chain_len(&self) -> usize {
        let used = self.used.load(Ordering::Relaxed);
        self.chains
            .iter()
            .take(used)
            .map(Chain::popcount)
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn armed_locations(&self) -> usize {
        let used = self.used.load(Ordering::Relaxed);
        self.chains
            .iter()
            .take(used)
            .filter(|c| c.armed.load(Ordering::Relaxed))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preseed_arms_recipient_without_admission() {
        let n = 4;
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(Vec::new(), vec![u16::MAX; n], vec![10, 0, 10, 10]);
        live.preseed_recipients();
        assert!(live.is_armed(10));
        assert_eq!(live.nearest_lower(10, 3), Some(2));
        assert_eq!(live.nearest_lower(10, 2), Some(0));
        assert!(live.admission_predecessor(3).is_none());
        assert_eq!(live.writer_state(10, 0).0, ST_PREDICTED);
        live.publish_write(0, 0, 10, false, |_| true);
        assert_eq!(live.writer_state(10, 0).0, ST_PUBLISHED);
        assert!(live.admission_predecessor(2).is_none());
    }

    #[test]
    fn rmw_predicts_class_and_admits_only_read_then_write() {
        let n = 3;
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(
            vec![ClassGroup {
                members: vec![0, 1, 2],
                contract: true,
            }],
            vec![0, 0, 0],
            vec![10, 10, 10],
        );
        live.publish_write(0, 0, 77, true, |_| true);
        assert!(live.is_armed(77));
        // No conflict yet: only the publisher is a writer. Classmates are not
        // predicted, and the class head does not run.
        assert_eq!(live.nearest_lower(77, 2), Some(0));
        assert_eq!(live.writer_state(77, 1).0, ST_EMPTY);
        assert!(live.admission_predecessor(2).is_none());
        assert!(live.class_head(2).is_none());
        live.note_class_conflict(0);
        live.note_conflict_writer(77, 0, |_| true);
        assert_eq!(live.nearest_lower(77, 2), Some(1));
        assert_eq!(live.writer_state(77, 1).0, ST_PREDICTED);
        // Evidence inserts the classmate but does not add an admission edge.
        // The class head is the barrier, and only after this conflict.
        assert!(live.admission_predecessor(2).is_none());
        assert_eq!(live.class_head(2), Some(0));
        assert!(live.class_head(0).is_none());

        let mut blind = LiveChain::new(n, ClassKeyKind::ToSelector);
        blind.install_classes(
            vec![ClassGroup {
                members: vec![0, 1, 2],
                contract: true,
            }],
            vec![0, 0, 0],
            vec![10, 10, 10],
        );
        blind.publish_write(0, 0, 88, false, |_| true);
        assert!(blind.is_armed(88));
        assert_eq!(blind.writer_state(88, 0).0, ST_PUBLISHED);
        assert_eq!(blind.writer_state(88, 1).0, ST_EMPTY);
        assert_eq!(blind.nearest_lower(88, 2), Some(0));
        assert!(blind.admission_predecessor(1).is_none());
        assert!(blind.admission_predecessor(2).is_none());

        let mut plain = LiveChain::new(n, ClassKeyKind::ToSelector);
        plain.install_classes(
            vec![ClassGroup {
                members: vec![0, 1, 2],
                contract: false,
            }],
            vec![0, 0, 0],
            vec![10, 10, 10],
        );
        assert!(plain.class_head(2).is_none());
    }
}
