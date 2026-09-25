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
}

struct Chain {
    member: Vec<AtomicU64>,
    radar: Vec<AtomicU64>,
    /// Packed state per tx: low 2 bits are the state, next 16 the incarnation.
    state: Vec<AtomicU64>,
    armed: AtomicBool,
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
    classes: Vec<ClassGroup>,
    class_hit: Vec<AtomicU64>,
    class_miss: Vec<AtomicU64>,
    class_open: Vec<AtomicBool>,
    membership: Vec<Mutex<SmallVec<[u16; 4]>>>,
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
            classes: Vec::new(),
            class_hit: Vec::new(),
            class_miss: Vec::new(),
            class_open: Vec::new(),
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
        }
    }

    /// Ignore this location in the ordered chains. Used for the beneficiary.
    pub(crate) fn skip_location(&self, location: u64) {
        self.skip.store(location, Ordering::Relaxed);
        self.skip_on.store(true, Ordering::Relaxed);
    }

    pub(crate) fn install_classes(&mut self, classes: Vec<ClassGroup>, class_of_tx: Vec<u16>) {
        let n = classes.len();
        self.class_hit = (0..n).map(|_| AtomicU64::new(1)).collect();
        self.class_miss = (0..n).map(|_| AtomicU64::new(1)).collect();
        self.class_open = (0..n).map(|_| AtomicBool::new(true)).collect();
        self.classes = classes;
        self.class_of_tx = class_of_tx;
    }

    /// Radar only. Does not set member bits and does not arm a wait.
    pub(crate) fn install_radar(&self, location: u64, txs: &[TxIdx]) {
        let Some(idx) = self.ensure_slot(location, 0) else {
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

    fn remember(&self, tx: TxIdx, slot: usize) {
        let mut mem = self.membership[tx].lock().unwrap();
        let slot = slot as u16;
        if !mem.contains(&slot) {
            mem.push(slot);
        }
    }

    fn ensure_slot(&self, location: u64, priority: usize) -> Option<usize> {
        {
            let dir = self.dir.read().unwrap();
            if let Some(&idx) = dir.index.get(&location) {
                return Some(idx);
            }
        }
        let mut dir = self.dir.write().unwrap();
        if let Some(&idx) = dir.index.get(&location) {
            return Some(idx);
        }
        let k_max = self.k_max.load(Ordering::Relaxed).min(K_LIMIT);
        let used = self.used.load(Ordering::Relaxed);
        if used < k_max {
            let idx = used;
            self.chains[idx].reset_slot();
            self.chains[idx].live.store(true, Ordering::Release);
            self.used.store(used + 1, Ordering::Relaxed);
            dir.index.insert(location, idx);
            self.any.store(true, Ordering::Release);
            return Some(idx);
        }
        // Replace the lowest-credit chain when the new location is hotter.
        let mut worst: Option<(usize, u64)> = None;
        for (idx, chain) in self.chains.iter().enumerate().take(used) {
            if !chain.live.load(Ordering::Relaxed) {
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
        if !self.any.load(Ordering::Relaxed) || tx == 0 {
            return None;
        }
        let mem = self.membership[tx].lock().unwrap();
        let mut best: Option<TxIdx> = None;
        for &slot in mem.iter() {
            let chain = &self.chains[slot as usize];
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
        let mem = self.membership[tx].lock().unwrap().clone();
        for slot in mem {
            let chain = &self.chains[slot as usize];
            let (state, _) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
            if state == ST_PREDICTED || state == ST_RUNNING {
                chain.state[tx].store(Chain::pack(ST_RUNNING, incarnation), Ordering::Release);
            }
        }
    }

    /// Publish one write into the ordered chain.
    ///
    /// Read-then-write joins the admission list so the writer waits for the
    /// previous final value. Blind and lazy writes are visible to readers
    /// (member bit, `Published`) and do not make writers wait on each other.
    /// A location enters the directory on the second writer, or on the first
    /// read-then-write of a repeated class.
    pub(crate) fn publish_write(
        &self,
        tx: TxIdx,
        incarnation: usize,
        location: u64,
        rmw: bool,
        tx_open: impl Fn(TxIdx) -> bool,
    ) {
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
        let open_for_class = rmw && class_len >= 2 && self.class_allows(class);
        let prev = seen.remove(&location);
        if prev.is_none() && !open_for_class {
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
        let Some(idx) = self.ensure_slot(location, priority) else {
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
            self.predict_siblings(idx, tx, class, &tx_open);
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
            self.remember(tx, idx);
        }
        if chain.writers.load(Ordering::Relaxed) >= 2 {
            chain.armed.store(true, Ordering::Release);
        }
    }

    fn predict_siblings(
        &self,
        idx: usize,
        tx: TxIdx,
        class: u16,
        tx_open: &impl Fn(TxIdx) -> bool,
    ) {
        let chain = &self.chains[idx];
        if chain.class_seeded.swap(true, Ordering::AcqRel) {
            return;
        }
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
                self.remember(member, idx);
            }
        }
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
                    .retain(|slot| *slot != idx as u16);
            }
        }
    }

    /// Validation failed on `location`. Backfill is the caller's mv scan.
    pub(crate) fn arm_failure(&self, location: u64) {
        if let Some(idx) = self.ensure_slot(location, K_LIMIT) {
            let chain = &self.chains[idx];
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
            let Some(idx) = self.ensure_slot(location, 8) else {
                continue;
            };
            let chain = &self.chains[idx];
            chain.set_member(tx);
            chain.state[tx].store(Chain::pack(ST_PREDICTED, incarnation), Ordering::Release);
            chain.armed.store(true, Ordering::Release);
            self.remember(tx, idx);
        }
    }

    /// Drop predicted bits the committed tx did not actually write, and wake is external.
    pub(crate) fn hole_clear(&self, tx: TxIdx, written: &[u64]) {
        let mem = self.membership[tx].lock().unwrap().clone();
        for slot in mem {
            let chain = &self.chains[slot as usize];
            let (state, inc) = Chain::unpack(chain.state[tx].load(Ordering::Acquire));
            if state == ST_PUBLISHED {
                let _ = (written, inc);
                continue;
            }
            chain.clear_member(tx);
            chain.state[tx].store(ST_EMPTY, Ordering::Release);
            if state == ST_PREDICTED {
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
            k -= 1;
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
