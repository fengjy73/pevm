#![allow(missing_docs)]
//! Fine-grain serial/OCC runtime tracing for lab analysis (opt-in).
//!
//! Disabled by default: zero cost on production paths when `Pevm::finegrain_trace`
//! is off. When enabled, parallel execution snapshots final RW sets (≈ G*),
//! location kinds, per-tx incarnation highs, and OCC abort events.
//!
//! **Deep mode** (`set_finegrain_deep(true)`): records producer-effect →
//! consumer-read `RawEffectEdge` on cold pevm `Database` MV reads.
//!
//! **Journal stream** (`set_finegrain_journal(true)`): implies deep + drives
//! opt-in `inspect_run` so every interpreter SLOAD/SSTORE/BALANCE/EXT* is
//! logged (including journal-cached repeats invisible to Db hooks), with live
//! write ordinals and mid-tx **gross-work** (gas_used_so_far / tx_gas_used).
//! Emits **one RawEffectEdge per cross-tx read instance** — no (p,c,ℓ) dedupe.
//! Research only — production default off.

use std::sync::Mutex;

use alloy_primitives::Address;
use serde::Serialize;

use crate::{
    MemoryEntry, MemoryLocation, MemoryValue, TxIdx, hash_deterministic, mv_memory::MvMemory,
    scheduler::Scheduler,
};

/// Location classification for analysis (not a TCB type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    Basic,
    BasicLazy,
    CodeHash,
    Storage,
    SelfDestructed,
    Unknown,
}

impl LocationKind {
    fn from_value(value: &MemoryValue) -> Self {
        match value {
            MemoryValue::Basic(_) => Self::Basic,
            MemoryValue::LazySender(_) | MemoryValue::LazyRecipient(_) => Self::BasicLazy,
            MemoryValue::CodeHash(_) => Self::CodeHash,
            MemoryValue::Storage(_) => Self::Storage,
            MemoryValue::SelfDestructed => Self::SelfDestructed,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::BasicLazy => "basic_lazy",
            Self::CodeHash => "code_hash",
            Self::Storage => "storage",
            Self::SelfDestructed => "selfdestruct",
            Self::Unknown => "unknown",
        }
    }
}

/// One successful validation abort (OCC ESTIMATE path).
#[derive(Debug, Clone, Serialize)]
pub struct AbortEvent {
    pub tx_idx: usize,
    pub incarnation: usize,
    pub n_write_locs: usize,
    /// Classic OCC: validations forced from `tx_idx+1` .. block_size.
    pub cascade_validations: usize,
}

/// Program vs handler class at effect grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    Program,
    Handler,
}

impl EffectClass {
    pub fn from_kind(kind: LocationKind) -> Self {
        match kind {
            LocationKind::Storage | LocationKind::CodeHash | LocationKind::SelfDestructed => {
                Self::Program
            }
            LocationKind::Basic | LocationKind::BasicLazy | LocationKind::Unknown => Self::Handler,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Handler => "handler",
        }
    }
}

/// One producer-effect → consumer-read observation (deep / journal mode).
///
/// **Instance semantics:** each cross-tx read that observes a foreign version
/// yields its own edge — including repeated reads of the same `(p,c,ℓ)`.
/// Collectors must never HashSet-dedupe on `(producer,consumer,location)`.
#[derive(Debug, Clone, Serialize)]
pub struct RawEffectEdge {
    pub producer_tx: usize,
    pub producer_effect_k: usize,
    pub producer_incarnation: usize,
    pub consumer_tx: usize,
    pub consumer_effect_k: usize,
    pub consumer_incarnation: usize,
    pub location: u64,
    pub kind: String,
    /// `program` | `handler`
    pub class: String,
    /// Cumulative gas billed in the consumer tx at this opcode (limit − remaining).
    pub gas_used_so_far: Option<u64>,
    /// Opcode/step counter if available (else mirrors consumer_effect_k).
    pub opcode_steps: Option<usize>,
    /// `gas_used_so_far` at this read — alias for gross-work numerator when set.
    pub gross_work_so_far: Option<u64>,
}

/// Per-consumer depth of first cross-tx program read (deep mode).
#[derive(Debug, Clone, Serialize)]
pub struct ConsumerFirstCross {
    pub tx_idx: usize,
    pub incarnation: usize,
    /// Effect ordinal of first program-class cross-tx MV read (None if none).
    pub first_program_cross_k: Option<usize>,
    pub first_program_cross_location: Option<u64>,
    pub first_program_producer_tx: Option<usize>,
    /// Total world-state DB / journal read effects observed this incarnation.
    pub total_db_effects: usize,
    /// Total journal write effects registered for this incarnation.
    pub total_write_effects: usize,
    pub gas_used: Option<u64>,
    pub gas_limit: Option<u64>,
    /// Gas billed at first program cross (numerator for gross-work depth).
    pub first_program_cross_gas: Option<u64>,
    /// Opcode steps at first program cross.
    pub first_program_cross_opcode_steps: Option<usize>,
    /// Total interpreter opcode steps this incarnation (inspect path).
    pub total_opcode_steps: Option<usize>,
    /// `first_program_cross_k / total_db_effects` when defined.
    pub depth_frac_effects: Option<f64>,
    /// Legacy: `gas_at_cross / gas_limit` (can dwarf true work fraction).
    pub depth_frac_gas: Option<f64>,
    /// **Preferred gross-work depth:** `gas_at_cross / tx_gas_used`.
    /// Formula: cumulative gas billed at first program-cross opcode
    /// divided by the tx's final billed gas (`exec_result.gas_used`).
    pub depth_frac_gross_work: Option<f64>,
    /// `opcode_steps_at_cross / total_opcode_steps` when both known.
    pub depth_frac_opcode: Option<f64>,
}

/// Per-transaction final RW (last committed incarnation).
#[derive(Debug, Clone, Serialize)]
pub struct TxRw {
    pub tx_idx: usize,
    pub incarnation: usize,
    pub reads: Vec<u64>,
    pub writes: Vec<u64>,
    pub n_reads: usize,
    pub n_writes: usize,
}

/// Machine-readable fine-grain snapshot after one parallel block.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FineGrainSnapshot {
    pub n_tx: usize,
    pub beneficiary_hash: u64,
    pub txs: Vec<TxRw>,
    /// location_hash → kind string (from last Data write observed in MV).
    pub location_kinds: Vec<(u64, String)>,
    pub abort_events: Vec<AbortEvent>,
    /// Final incarnation index per tx (0 = first success, k = k reexecs before success).
    pub final_incarnations: Vec<usize>,
    /// Deep-mode effect edges (empty when deep off).
    pub effect_edges: Vec<RawEffectEdge>,
    /// Deep-mode per-consumer first cross-tx program depth (empty when deep off).
    pub consumer_first_cross: Vec<ConsumerFirstCross>,
    /// Whether deep instrumentation was active for this capture.
    pub deep_mode: bool,
    /// Whether interpreter/journal stream (vs Db-hook-only) was active.
    pub journal_mode: bool,
    /// Definitional-gap counters (journal/deep research).
    pub stream_diag: EffectStreamDiag,
}

/// Instrumentation for RAW count / depth definitional gaps (research).
#[derive(Debug, Clone, Default, Serialize)]
pub struct EffectStreamDiag {
    /// Inspector SLOAD/BALANCE/EXT*/SELFBALANCE instances.
    pub journal_reads: usize,
    /// Of which: emitted a cross-tx RawEffectEdge (slot/account location RAW).
    pub journal_reads_cross: usize,
    /// Journal reads with no prior writer on that exact location.
    pub journal_reads_no_prior_writer: usize,
    /// SLOAD instances where account had a prior writer but slot did not
    /// (candidate user-count inflation under account-grain RAW).
    pub sload_account_grain_cross: usize,
    /// Live SSTORE instances.
    pub journal_sstore: usize,
    /// Live account-write instances (valued CALL / CREATE / SELFDESTRUCT).
    pub journal_account_writes: usize,
    /// Finalize write_set fills for locations not live-noted.
    pub finalize_write_fills: usize,
    /// Edges pushed (pre filter); should equal journal_reads_cross + db-hook edges.
    pub edges_pushed: usize,
    /// Max repeats of the same (producer_tx, consumer_tx, location) in edges.
    pub max_pcl_repeats: usize,
    /// Mean edges / unique (p,c,ℓ); ≈1 implies few repeated foreign reads.
    pub mean_effects_per_pcl: f64,
}

/// Opt-in collector living on [`crate::Pevm`].
#[derive(Debug, Default)]
pub struct FineGrainCollector {
    abort_events: Mutex<Vec<AbortEvent>>,
    last_snapshot: Mutex<Option<FineGrainSnapshot>>,
    /// Research deep mode: effect-level RAW edges + depth (off by default).
    deep: std::sync::atomic::AtomicBool,
    /// Research journal/inspector stream (implies deep; off by default).
    journal: std::sync::atomic::AtomicBool,
    deep_state: Mutex<DeepRuntimeState>,
}

/// Mutable deep-mode runtime state (research path only).
#[derive(Debug, Default)]
struct DeepRuntimeState {
    /// (tx, incarnation, location) → **latest** effect_k (live ordinal when journal; else finalize).
    /// Multiple writes to the same ℓ keep distinct live ordinals via `live_write_counters`;
    /// this map only stores the latest version for producer resolve (not edge dedupe).
    write_effects: std::collections::HashMap<(usize, usize, u64), (usize, LocationKind)>,
    /// Last writer per location across the block (live map for journal producer resolve).
    last_writer: std::collections::HashMap<u64, (usize, usize, usize, LocationKind)>,
    /// Last writer per account address (any Basic/Storage touch) — account-grain diag.
    last_account_writer: std::collections::HashMap<[u8; 20], (usize, usize, usize)>,
    /// Next live write ordinal per (tx, incarnation) — increments on **every** write instance.
    live_write_counters: std::collections::HashMap<(usize, usize), usize>,
    /// Per executing consumer live depth state.
    consumers: std::collections::HashMap<usize, ConsumerLive>,
    edges: Vec<RawEffectEdge>,
    finished: Vec<ConsumerFirstCross>,
    diag: EffectStreamDiag,
}

#[derive(Debug, Clone)]
struct ConsumerLive {
    incarnation: usize,
    db_effects: usize,
    first_program_cross_k: Option<usize>,
    first_program_cross_location: Option<u64>,
    first_program_producer_tx: Option<usize>,
    /// Gas used (limit - remaining) at first program cross, when known.
    first_program_cross_gas: Option<u64>,
    first_program_cross_opcode_steps: Option<usize>,
    write_effects: usize,
    gas_used: Option<u64>,
    gas_limit: Option<u64>,
}

impl ConsumerLive {
    fn new(incarnation: usize, gas_limit: Option<u64>) -> Self {
        Self {
            incarnation,
            db_effects: 0,
            first_program_cross_k: None,
            first_program_cross_location: None,
            first_program_producer_tx: None,
            first_program_cross_gas: None,
            first_program_cross_opcode_steps: None,
            write_effects: 0,
            gas_used: None,
            gas_limit,
        }
    }
}

impl FineGrainCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_deep(&self, enabled: bool) {
        self.deep
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn deep_enabled(&self) -> bool {
        self.deep.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_journal(&self, enabled: bool) {
        self.journal
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        if enabled {
            self.deep
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn journal_enabled(&self) -> bool {
        self.journal.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.abort_events.lock().unwrap().clear();
        *self.last_snapshot.lock().unwrap() = None;
        *self.deep_state.lock().unwrap() = DeepRuntimeState::default();
    }

    pub fn record_abort(
        &self,
        tx_idx: TxIdx,
        incarnation: usize,
        n_write_locs: usize,
        cascade_validations: usize,
    ) {
        self.abort_events.lock().unwrap().push(AbortEvent {
            tx_idx,
            incarnation,
            n_write_locs,
            cascade_validations,
        });
    }

    /// Start (or reset) deep counters for a consumer incarnation.
    pub(crate) fn deep_begin_consumer(&self, tx_idx: TxIdx, incarnation: usize) {
        if !self.deep_enabled() {
            return;
        }
        let mut st = self.deep_state.lock().unwrap();
        st.consumers
            .insert(tx_idx, ConsumerLive::new(incarnation, None));
    }

    /// Register write-set effects for a producer incarnation.
    /// Journal mode: keep live SSTORE/account ordinals; only fill missing finalize locs.
    pub(crate) fn deep_register_writes(
        &self,
        tx_idx: TxIdx,
        incarnation: usize,
        write_set: &[(crate::MemoryLocationHash, crate::MemoryValue)],
    ) {
        if !self.deep_enabled() {
            return;
        }
        let journal = self.journal_enabled();
        let mut st = self.deep_state.lock().unwrap();
        let mut next_k = st
            .live_write_counters
            .get(&(tx_idx, incarnation))
            .copied()
            .unwrap_or(0);
        for (finalize_k, (loc, value)) in write_set.iter().enumerate() {
            let kind = LocationKind::from_value(value);
            let key = (tx_idx, incarnation, *loc);
            if journal {
                if let Some(&(k, _)) = st.write_effects.get(&key) {
                    st.last_writer.insert(*loc, (tx_idx, incarnation, k, kind));
                    continue;
                }
                let k = next_k;
                next_k += 1;
                st.write_effects.insert(key, (k, kind));
                st.last_writer.insert(*loc, (tx_idx, incarnation, k, kind));
                st.diag.finalize_write_fills += 1;
            } else {
                st.write_effects.insert(key, (finalize_k, kind));
                st.last_writer
                    .insert(*loc, (tx_idx, incarnation, finalize_k, kind));
            }
        }
        if journal {
            st.live_write_counters
                .insert((tx_idx, incarnation), next_k);
        }
        if let Some(c) = st.consumers.get_mut(&tx_idx) {
            if c.incarnation == incarnation {
                c.write_effects = if journal {
                    next_k
                } else {
                    write_set.len()
                };
            }
        }
    }

    /// Observe one world-state DB read; emit RawEffectEdge if origin is prior tx.
    /// When `bump_depth` is false, emit edge only (lazy-chain extra producers).
    /// **No (p,c,ℓ) dedupe** — every observing read instance pushes an edge.
    pub(crate) fn deep_note_db_read(
        &self,
        consumer_tx: TxIdx,
        consumer_incarnation: usize,
        producer: Option<(TxIdx, usize)>,
        location: crate::MemoryLocationHash,
        kind_hint: LocationKind,
        bump_depth: bool,
    ) {
        if !self.deep_enabled() {
            return;
        }
        if self.journal_enabled() && kind_hint == LocationKind::Storage {
            return;
        }
        let mut st = self.deep_state.lock().unwrap();

        let producer_meta = producer.and_then(|(producer_tx, producer_inc)| {
            if producer_tx >= consumer_tx {
                return None;
            }
            let (producer_k, kind) = st
                .write_effects
                .get(&(producer_tx, producer_inc, location))
                .copied()
                .unwrap_or((0, kind_hint));
            Some((producer_tx, producer_inc, producer_k, kind))
        });

        let c = st
            .consumers
            .entry(consumer_tx)
            .or_insert_with(|| ConsumerLive::new(consumer_incarnation, None));
        if c.incarnation != consumer_incarnation {
            *c = ConsumerLive::new(consumer_incarnation, None);
        }
        let consumer_k = if bump_depth {
            let k = c.db_effects;
            c.db_effects += 1;
            k
        } else {
            c.db_effects.saturating_sub(1)
        };

        let Some((producer_tx, producer_inc, producer_k, kind)) = producer_meta else {
            return;
        };
        let class = EffectClass::from_kind(kind);
        if class == EffectClass::Program && c.first_program_cross_k.is_none() {
            c.first_program_cross_k = Some(consumer_k);
            c.first_program_cross_location = Some(location);
            c.first_program_producer_tx = Some(producer_tx);
        }
        let edge = RawEffectEdge {
            producer_tx,
            producer_effect_k: producer_k,
            producer_incarnation: producer_inc,
            consumer_tx,
            consumer_effect_k: consumer_k,
            consumer_incarnation,
            location,
            kind: kind.as_str().to_string(),
            class: class.as_str().to_string(),
            gas_used_so_far: None,
            opcode_steps: Some(consumer_k),
            gross_work_so_far: None,
        };
        let _ = c;
        st.edges.push(edge);
        st.diag.edges_pushed += 1;
    }

    /// Live SSTORE / account write: bump live effect ordinal + last_writer every instance.
    pub(crate) fn deep_note_journal_write(
        &self,
        tx_idx: TxIdx,
        incarnation: usize,
        location: crate::MemoryLocationHash,
        kind: LocationKind,
        gas_used_so_far: Option<u64>,
        gas_limit: Option<u64>,
        opcode_steps: Option<usize>,
        account: Option<[u8; 20]>,
    ) -> usize {
        if !self.journal_enabled() {
            return 0;
        }
        let mut st = self.deep_state.lock().unwrap();
        let k = {
            let ctr = st.live_write_counters.entry((tx_idx, incarnation)).or_insert(0);
            let k = *ctr;
            *ctr += 1;
            k
        };
        st.write_effects
            .insert((tx_idx, incarnation, location), (k, kind));
        st.last_writer
            .insert(location, (tx_idx, incarnation, k, kind));
        if let Some(acct) = account {
            st.last_account_writer
                .insert(acct, (tx_idx, incarnation, k));
        }
        match kind {
            LocationKind::Storage => st.diag.journal_sstore += 1,
            LocationKind::Basic | LocationKind::BasicLazy | LocationKind::CodeHash => {
                st.diag.journal_account_writes += 1
            }
            _ => {}
        }
        if let Some(c) = st.consumers.get_mut(&tx_idx) {
            if c.incarnation == incarnation {
                c.write_effects = k + 1;
                if c.gas_limit.is_none() {
                    c.gas_limit = gas_limit;
                }
                let _ = (gas_used_so_far, opcode_steps);
            }
        }
        k
    }

    /// Live account write (valued CALL / CREATE / SELFDESTRUCT).
    pub(crate) fn deep_note_journal_account_write(
        &self,
        tx_idx: TxIdx,
        incarnation: usize,
        address: Address,
        kind: LocationKind,
        gas_used_so_far: Option<u64>,
        gas_limit: Option<u64>,
        opcode_steps: Option<usize>,
    ) -> usize {
        let loc = hash_deterministic(MemoryLocation::Basic(address));
        let mut acct = [0u8; 20];
        acct.copy_from_slice(address.as_slice());
        self.deep_note_journal_write(
            tx_idx,
            incarnation,
            loc,
            kind,
            gas_used_so_far,
            gas_limit,
            opcode_steps,
            Some(acct),
        )
    }

    /// Interpreter/journal read: one RawEffectEdge per foreign observe instance (no pcl dedupe).
    pub(crate) fn deep_note_journal_read(
        &self,
        consumer_tx: TxIdx,
        consumer_incarnation: usize,
        location: crate::MemoryLocationHash,
        kind_hint: LocationKind,
        gas_used_so_far: Option<u64>,
        gas_limit: Option<u64>,
        opcode_steps: Option<usize>,
        account: Option<[u8; 20]>,
    ) {
        if !self.journal_enabled() {
            return;
        }
        let mut st = self.deep_state.lock().unwrap();
        st.diag.journal_reads += 1;

        let producer_meta = st.last_writer.get(&location).and_then(|&(ptx, pinc, pk, kind)| {
            if ptx >= consumer_tx {
                None
            } else {
                let kind = if kind == LocationKind::Unknown {
                    kind_hint
                } else {
                    kind
                };
                Some((ptx, pinc, pk, kind))
            }
        });

        if producer_meta.is_none() {
            if let Some(acct) = account {
                if let Some(&(ptx, _, _)) = st.last_account_writer.get(&acct) {
                    if ptx < consumer_tx {
                        st.diag.sload_account_grain_cross += 1;
                    }
                }
            }
        }

        let c = st
            .consumers
            .entry(consumer_tx)
            .or_insert_with(|| ConsumerLive::new(consumer_incarnation, gas_limit));
        if c.incarnation != consumer_incarnation {
            *c = ConsumerLive::new(consumer_incarnation, gas_limit);
        }
        if c.gas_limit.is_none() {
            c.gas_limit = gas_limit;
        }
        let consumer_k = c.db_effects;
        c.db_effects += 1;

        let Some((producer_tx, producer_inc, producer_k, kind)) = producer_meta else {
            st.diag.journal_reads_no_prior_writer += 1;
            return;
        };
        let class = EffectClass::from_kind(kind);
        if class == EffectClass::Program && c.first_program_cross_k.is_none() {
            c.first_program_cross_k = Some(consumer_k);
            c.first_program_cross_location = Some(location);
            c.first_program_producer_tx = Some(producer_tx);
            c.first_program_cross_gas = gas_used_so_far;
            c.first_program_cross_opcode_steps = opcode_steps;
        }
        let edge = RawEffectEdge {
            producer_tx,
            producer_effect_k: producer_k,
            producer_incarnation: producer_inc,
            consumer_tx,
            consumer_effect_k: consumer_k,
            consumer_incarnation,
            location,
            kind: kind.as_str().to_string(),
            class: class.as_str().to_string(),
            gas_used_so_far,
            opcode_steps: opcode_steps.or(Some(consumer_k)),
            gross_work_so_far: gas_used_so_far,
        };
        let _ = c;
        st.diag.journal_reads_cross += 1;
        st.edges.push(edge);
        st.diag.edges_pushed += 1;
    }

    /// Finalize consumer depth after incarnation.
    /// Gross-work depth = gas_at_first_program_cross / tx_gas_used (preferred for EV).
    pub(crate) fn deep_finish_consumer(
        &self,
        tx_idx: TxIdx,
        incarnation: usize,
        gas_used: Option<u64>,
        gas_limit: Option<u64>,
        total_opcode_steps: Option<usize>,
    ) {
        if !self.deep_enabled() {
            return;
        }
        let mut st = self.deep_state.lock().unwrap();
        let Some(c) = st.consumers.remove(&tx_idx) else {
            return;
        };
        if c.incarnation != incarnation {
            return;
        }
        let depth_frac_effects = c.first_program_cross_k.map(|k| {
            if c.db_effects == 0 {
                0.0
            } else {
                k as f64 / c.db_effects as f64
            }
        });
        let gl = gas_limit.or(c.gas_limit);
        let gu = gas_used.or(c.gas_used);
        let depth_frac_gas = match (c.first_program_cross_gas, gl) {
            (Some(g), Some(limit)) if limit > 0 => Some((g as f64 / limit as f64).min(1.0)),
            _ => None,
        };
        let depth_frac_gross_work = match (c.first_program_cross_gas, gu) {
            (Some(g), Some(total)) if total > 0 => Some((g as f64 / total as f64).min(1.0)),
            _ => None,
        };
        let depth_frac_opcode = match (c.first_program_cross_opcode_steps, total_opcode_steps) {
            (Some(s), Some(total)) if total > 0 => Some((s as f64 / total as f64).min(1.0)),
            _ => None,
        };
        st.finished.push(ConsumerFirstCross {
            tx_idx,
            incarnation,
            first_program_cross_k: c.first_program_cross_k,
            first_program_cross_location: c.first_program_cross_location,
            first_program_producer_tx: c.first_program_producer_tx,
            total_db_effects: c.db_effects,
            total_write_effects: c.write_effects,
            gas_used: gu,
            gas_limit: gl,
            first_program_cross_gas: c.first_program_cross_gas,
            first_program_cross_opcode_steps: c.first_program_cross_opcode_steps,
            total_opcode_steps,
            depth_frac_effects,
            depth_frac_gas,
            depth_frac_gross_work,
            depth_frac_opcode,
        });
    }

    pub(crate) fn capture(
        &self,
        mv: &MvMemory,
        scheduler: &Scheduler,
        beneficiary: Address,
    ) {
        let n_tx = scheduler.block_size();
        let beneficiary_hash = hash_deterministic(MemoryLocation::Basic(beneficiary));
        let final_incarnations = scheduler.incarnation_snapshot();
        let mut txs = Vec::with_capacity(n_tx);
        for tx_idx in 0..n_tx {
            let reads = mv.read_locations(tx_idx);
            let writes = mv.write_locations(tx_idx);
            txs.push(TxRw {
                tx_idx,
                incarnation: final_incarnations.get(tx_idx).copied().unwrap_or(0),
                n_reads: reads.len(),
                n_writes: writes.len(),
                reads,
                writes,
            });
        }

        let mut location_kinds = Vec::new();
        for entry in mv.data.iter() {
            let loc = *entry.key();
            let kind = entry
                .value()
                .iter()
                .rev()
                .find_map(|(_, mem)| match mem {
                    MemoryEntry::Data(_, value) => Some(LocationKind::from_value(value)),
                    MemoryEntry::Estimate => None,
                })
                .unwrap_or(LocationKind::Unknown);
            location_kinds.push((loc, kind.as_str().to_string()));
        }
        location_kinds.sort_by_key(|(h, _)| *h);

        let abort_events = self.abort_events.lock().unwrap().clone();
        let deep_mode = self.deep_enabled();
        let journal_mode = self.journal_enabled();
        let (effect_edges, consumer_first_cross, mut stream_diag) = if deep_mode {
            let st = self.deep_state.lock().unwrap();
            (st.edges.clone(), st.finished.clone(), st.diag.clone())
        } else {
            (Vec::new(), Vec::new(), EffectStreamDiag::default())
        };
        if !effect_edges.is_empty() {
            use std::collections::HashMap;
            let mut pcl: HashMap<(usize, usize, u64), usize> = HashMap::new();
            for e in &effect_edges {
                *pcl.entry((e.producer_tx, e.consumer_tx, e.location))
                    .or_default() += 1;
            }
            stream_diag.max_pcl_repeats = pcl.values().copied().max().unwrap_or(0);
            let n_pcl = pcl.len();
            stream_diag.mean_effects_per_pcl = if n_pcl == 0 {
                0.0
            } else {
                effect_edges.len() as f64 / n_pcl as f64
            };
        }
        *self.last_snapshot.lock().unwrap() = Some(FineGrainSnapshot {
            n_tx,
            beneficiary_hash,
            txs,
            location_kinds,
            abort_events,
            final_incarnations,
            effect_edges,
            consumer_first_cross,
            deep_mode,
            journal_mode,
            stream_diag,
        });
    }

    pub fn take_snapshot(&self) -> Option<FineGrainSnapshot> {
        self.last_snapshot.lock().unwrap().take()
    }

    pub fn peek_snapshot(&self) -> Option<FineGrainSnapshot> {
        self.last_snapshot.lock().unwrap().clone()
    }
}

/// Build RAW/WAW dependency edges from a snapshot.
///
/// `exclude_beneficiary`: drop coinbase Basic hash (lazy evaluated).
/// `exclude_basic_lazy`: drop LazySender/LazyRecipient locations — pevm evaluates
/// these at block end, so they are **not** on the speculative critical path.
pub fn dependency_edges(
    snap: &FineGrainSnapshot,
    exclude_beneficiary: bool,
    exclude_basic_lazy: bool,
) -> Vec<(usize, usize, u64, &'static str)> {
    use std::collections::{HashMap, HashSet};

    let lazy: HashSet<u64> = if exclude_basic_lazy {
        snap.location_kinds
            .iter()
            .filter(|(_, k)| k == "basic_lazy")
            .map(|(h, _)| *h)
            .collect()
    } else {
        HashSet::new()
    };

    let mut writers: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut readers: HashMap<u64, Vec<usize>> = HashMap::new();
    for tx in &snap.txs {
        for &w in &tx.writes {
            if exclude_beneficiary && w == snap.beneficiary_hash {
                continue;
            }
            if lazy.contains(&w) {
                continue;
            }
            writers.entry(w).or_default().push(tx.tx_idx);
        }
        for &r in &tx.reads {
            if exclude_beneficiary && r == snap.beneficiary_hash {
                continue;
            }
            if lazy.contains(&r) {
                continue;
            }
            readers.entry(r).or_default().push(tx.tx_idx);
        }
    }
    for v in writers.values_mut() {
        v.sort_unstable();
        v.dedup();
    }
    for v in readers.values_mut() {
        v.sort_unstable();
        v.dedup();
    }

    let mut edges = Vec::new();
    let mut edge_set: HashSet<(usize, usize, u64)> = HashSet::new();

    // WAW: consecutive writers on same location
    for (&loc, ws) in &writers {
        for pair in ws.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a < b && edge_set.insert((a, b, loc)) {
                edges.push((a, b, loc, "waw"));
            }
        }
    }
    // RAW: each reader depends on closest lower writer
    for (&loc, rs) in &readers {
        let Some(ws) = writers.get(&loc) else { continue };
        for &r in rs {
            if let Some(&w) = ws.iter().rev().find(|&&w| w < r) {
                if edge_set.insert((w, r, loc)) {
                    edges.push((w, r, loc, "raw"));
                }
            }
        }
    }
    edges
}

/// Summary DAG stats from edges + n_tx.
#[derive(Debug, Clone, Serialize)]
pub struct DagStats {
    pub n_edges: usize,
    pub n_raw: usize,
    pub n_waw: usize,
    pub longest_chain: usize,
    pub independent_txs: usize,
    pub independent_frac: f64,
    pub max_wave_width: usize,
    pub mean_wave_width: f64,
    pub n_levels: usize,
    pub multi_writer_locs: usize,
    pub max_writers_on_loc: usize,
    pub conflict_component_sizes_top10: Vec<usize>,
    pub max_conflict_component: usize,
}

pub fn analyze_dag(
    snap: &FineGrainSnapshot,
    exclude_beneficiary: bool,
    exclude_basic_lazy: bool,
) -> DagStats {
    use std::collections::{HashMap, HashSet, VecDeque};

    let edges = dependency_edges(snap, exclude_beneficiary, exclude_basic_lazy);
    let n_tx = snap.n_tx;
    let n_raw = edges.iter().filter(|e| e.3 == "raw").count();
    let n_waw = edges.iter().filter(|e| e.3 == "waw").count();

    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n_tx];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n_tx];
    let mut undirected: Vec<HashSet<usize>> = vec![HashSet::new(); n_tx];
    for &(a, b, _, _) in &edges {
        if a < n_tx && b < n_tx {
            preds[b].push(a);
            succs[a].push(b);
            undirected[a].insert(b);
            undirected[b].insert(a);
        }
    }
    for p in &mut preds {
        p.sort_unstable();
        p.dedup();
    }
    for s in &mut succs {
        s.sort_unstable();
        s.dedup();
    }

    // Longest path (unit weight) via DP in index order (edges always a < b).
    let mut dist = vec![1usize; n_tx];
    for b in 0..n_tx {
        for &a in &preds[b] {
            dist[b] = dist[b].max(dist[a] + 1);
        }
    }
    let longest_chain = dist.iter().copied().max().unwrap_or(0);

    let independent_txs = (0..n_tx)
        .filter(|&t| preds[t].is_empty() && succs[t].is_empty())
        .count();

    // Wave levels = dist; width = count per level value
    let mut width: HashMap<usize, usize> = HashMap::new();
    for &d in &dist {
        *width.entry(d).or_default() += 1;
    }
    let max_wave_width = width.values().copied().max().unwrap_or(0);
    let mean_wave_width = if width.is_empty() {
        0.0
    } else {
        width.values().sum::<usize>() as f64 / width.len() as f64
    };

    let lazy_set: HashSet<u64> = if exclude_basic_lazy {
        snap.location_kinds
            .iter()
            .filter(|(_, k)| k == "basic_lazy")
            .map(|(h, _)| *h)
            .collect()
    } else {
        HashSet::new()
    };
    let mut uniq_writers: HashMap<u64, HashSet<usize>> = HashMap::new();
    for tx in &snap.txs {
        for &w in &tx.writes {
            if exclude_beneficiary && w == snap.beneficiary_hash {
                continue;
            }
            if lazy_set.contains(&w) {
                continue;
            }
            uniq_writers.entry(w).or_default().insert(tx.tx_idx);
        }
    }
    let multi_writer_locs = uniq_writers.values().filter(|s| s.len() >= 2).count();
    let max_writers_on_loc = uniq_writers.values().map(|s| s.len()).max().unwrap_or(0);

    // Connected components on undirected conflict graph
    let mut seen = vec![false; n_tx];
    let mut sizes = Vec::new();
    for start in 0..n_tx {
        if seen[start] || undirected[start].is_empty() {
            if !seen[start] {
                seen[start] = true;
            }
            continue;
        }
        let mut q = VecDeque::new();
        q.push_back(start);
        seen[start] = true;
        let mut sz = 0usize;
        while let Some(u) = q.pop_front() {
            sz += 1;
            for &v in &undirected[u] {
                if !seen[v] {
                    seen[v] = true;
                    q.push_back(v);
                }
            }
        }
        if sz > 1 {
            sizes.push(sz);
        }
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    let max_conflict_component = sizes.first().copied().unwrap_or(0);
    let conflict_component_sizes_top10 = sizes.into_iter().take(10).collect();

    DagStats {
        n_edges: edges.len(),
        n_raw,
        n_waw,
        longest_chain,
        independent_txs,
        independent_frac: if n_tx == 0 {
            0.0
        } else {
            independent_txs as f64 / n_tx as f64
        },
        max_wave_width,
        mean_wave_width,
        n_levels: width.len(),
        multi_writer_locs,
        max_writers_on_loc,
        conflict_component_sizes_top10,
        max_conflict_component,
    }
}

/// Hot locations by (n_writers + n_readers) touch count.
#[derive(Debug, Clone, Serialize)]
pub struct HotLocation {
    pub location: u64,
    pub kind: String,
    pub n_writers: usize,
    pub n_readers: usize,
    pub touches: usize,
}

pub fn hot_locations(snap: &FineGrainSnapshot, top_k: usize, exclude_beneficiary: bool) -> Vec<HotLocation> {
    use std::collections::{HashMap, HashSet};

    let kind_map: HashMap<u64, String> = snap.location_kinds.iter().cloned().collect();
    let mut writers: HashMap<u64, HashSet<usize>> = HashMap::new();
    let mut readers: HashMap<u64, HashSet<usize>> = HashMap::new();
    for tx in &snap.txs {
        for &w in &tx.writes {
            if exclude_beneficiary && w == snap.beneficiary_hash {
                continue;
            }
            writers.entry(w).or_default().insert(tx.tx_idx);
        }
        for &r in &tx.reads {
            if exclude_beneficiary && r == snap.beneficiary_hash {
                continue;
            }
            readers.entry(r).or_default().insert(tx.tx_idx);
        }
    }
    let mut all: HashSet<u64> = writers.keys().copied().collect();
    all.extend(readers.keys().copied());
    let mut hot: Vec<HotLocation> = all
        .into_iter()
        .map(|loc| {
            let nw = writers.get(&loc).map(|s| s.len()).unwrap_or(0);
            let nr = readers.get(&loc).map(|s| s.len()).unwrap_or(0);
            HotLocation {
                location: loc,
                kind: kind_map
                    .get(&loc)
                    .cloned()
                    .unwrap_or_else(|| LocationKind::Unknown.as_str().to_string()),
                n_writers: nw,
                n_readers: nr,
                touches: nw + nr,
            }
        })
        .collect();
    hot.sort_by(|a, b| b.touches.cmp(&a.touches).then(b.n_writers.cmp(&a.n_writers)));
    hot.truncate(top_k);
    hot
}

/// Kind histogram over written locations (and known kinds).
pub fn kind_histogram(snap: &FineGrainSnapshot) -> std::collections::HashMap<String, usize> {
    let mut h = std::collections::HashMap::new();
    for (_, kind) in &snap.location_kinds {
        *h.entry(kind.clone()).or_default() += 1;
    }
    h
}

/// One RAW producer→consumer edge with location kind + program/handler class.
#[derive(Debug, Clone, Serialize)]
pub struct RawEdge {
    pub producer_tx: usize,
    pub consumer_tx: usize,
    pub location: u64,
    pub kind: String,
    /// `program` = storage/code_hash/selfdestruct; `handler` = basic/basic_lazy/unknown.
    pub class: String,
    pub producer_lag: usize,
}

fn edge_class(kind: &str) -> &'static str {
    match kind {
        "storage" | "code_hash" | "selfdestruct" => "program",
        _ => "handler",
    }
}

/// RAW edges only (effective G*: exclude beneficiary + basic_lazy by default).
pub fn classify_raw_edges(
    snap: &FineGrainSnapshot,
    exclude_beneficiary: bool,
    exclude_basic_lazy: bool,
) -> Vec<RawEdge> {
    use std::collections::HashMap;

    let kind_map: HashMap<u64, String> = snap.location_kinds.iter().cloned().collect();
    let edges = dependency_edges(snap, exclude_beneficiary, exclude_basic_lazy);
    edges
        .into_iter()
        .filter(|(_, _, _, t)| *t == "raw")
        .map(|(p, c, loc, _)| {
            let kind = kind_map
                .get(&loc)
                .cloned()
                .unwrap_or_else(|| LocationKind::Unknown.as_str().to_string());
            let class = edge_class(&kind).to_string();
            RawEdge {
                producer_tx: p,
                consumer_tx: c,
                location: loc,
                kind,
                class,
                producer_lag: c.saturating_sub(p),
            }
        })
        .collect()
}

/// Longest dependency path using only program-class RAW edges (unit weight).
pub fn program_raw_longest_chain(raw: &[RawEdge]) -> usize {
    if raw.is_empty() {
        return 0;
    }
    let mut max_tx = 0usize;
    for e in raw {
        max_tx = max_tx.max(e.producer_tx).max(e.consumer_tx);
    }
    let n = max_tx + 1;
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in raw {
        if e.class != "program" {
            continue;
        }
        preds[e.consumer_tx].push(e.producer_tx);
    }
    for p in &mut preds {
        p.sort_unstable();
        p.dedup();
    }
    let mut dist = vec![1usize; n];
    for b in 0..n {
        for &a in &preds[b] {
            dist[b] = dist[b].max(dist[a] + 1);
        }
    }
    dist.into_iter().max().unwrap_or(0)
}


/// Summarize deep effect-RAW edges (exclude beneficiary / basic_lazy when requested).
pub fn filter_effect_edges(
    snap: &FineGrainSnapshot,
    exclude_beneficiary: bool,
    exclude_basic_lazy: bool,
) -> Vec<&RawEffectEdge> {
    snap.effect_edges
        .iter()
        .filter(|e| {
            if exclude_beneficiary && e.location == snap.beneficiary_hash {
                return false;
            }
            if exclude_basic_lazy && e.kind == "basic_lazy" {
                return false;
            }
            true
        })
        .collect()
}

/// Longest path on effect-edge DAG projected to txs (program-only option).
pub fn effect_raw_longest_chain(edges: &[&RawEffectEdge], program_only: bool) -> usize {
    if edges.is_empty() {
        return 0;
    }
    let mut max_tx = 0usize;
    for e in edges {
        if program_only && e.class != "program" {
            continue;
        }
        max_tx = max_tx.max(e.producer_tx).max(e.consumer_tx);
    }
    let n = max_tx + 1;
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        if program_only && e.class != "program" {
            continue;
        }
        preds[e.consumer_tx].push(e.producer_tx);
    }
    for p in &mut preds {
        p.sort_unstable();
        p.dedup();
    }
    let mut dist = vec![1usize; n];
    for b in 0..n {
        for &a in &preds[b] {
            dist[b] = dist[b].max(dist[a] + 1);
        }
    }
    dist.into_iter().max().unwrap_or(0)
}

/// Max outbound fan-out (distinct consumers) from any producer on effect edges.
pub fn effect_raw_max_fanout(edges: &[&RawEffectEdge], program_only: bool) -> usize {
    use std::collections::{HashMap, HashSet};
    let mut m: HashMap<usize, HashSet<usize>> = HashMap::new();
    for e in edges {
        if program_only && e.class != "program" {
            continue;
        }
        m.entry(e.producer_tx).or_default().insert(e.consumer_tx);
    }
    m.values().map(|s| s.len()).max().unwrap_or(0)
}

/// Percentile of a sorted f64 slice.
pub fn percentile_f64(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// M-A vs M-D style redo/wait proxies from deep consumer depths + effect edges.
///
/// Units are **normalized consumer-work** (1.0 = full consumer db-effect budget).
/// Rough: not wall-clock calibrated.
#[derive(Debug, Clone, Serialize)]
pub struct MaMdProxy {
    pub n_consumers_with_program_cross: usize,
    /// M-A: always SpecRead → conflict pays full remaining = 1.0 per such consumer.
    pub ma_redo_cost: f64,
    /// M-D: redo ∝ (1 − d) using depth_frac_effects at first program cross.
    pub md_redo_cost: f64,
    /// Estimated redo saved vs M-A: sum(d) over consumers.
    pub redo_saved: f64,
    /// Wait added proxy: sum over unique program pairs of lag_norm * (1 − steal).
    pub wait_added: f64,
    pub depth_p10: f64,
    pub depth_p50: f64,
    pub depth_p90: f64,
    pub depth_mean: f64,
    pub frac_depth_lt_0_01: f64,
}

pub fn estimate_ma_md(
    snap: &FineGrainSnapshot,
    edges: &[&RawEffectEdge],
    n_tx: usize,
) -> MaMdProxy {
    use std::collections::{HashMap, HashSet};

    let mut best: HashMap<usize, &ConsumerFirstCross> = HashMap::new();
    for c in &snap.consumer_first_cross {
        best.entry(c.tx_idx)
            .and_modify(|e| {
                if c.incarnation >= e.incarnation {
                    *e = c;
                }
            })
            .or_insert(c);
    }
    // Prefer gross-work (gas_at_cross/tx_gas_used), then gas/limit, then effect ordinal.
    let mut depths: Vec<f64> = best
        .values()
        .filter_map(|c| {
            c.depth_frac_gross_work
                .or(c.depth_frac_gas)
                .or(c.depth_frac_effects)
        })
        .collect();
    depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n_c = depths.len();
    let ma_redo = n_c as f64;
    let md_redo: f64 = depths.iter().map(|d| (1.0 - d).max(0.0)).sum();
    let redo_saved = ma_redo - md_redo;
    let mean = if n_c == 0 {
        0.0
    } else {
        depths.iter().sum::<f64>() / n_c as f64
    };
    let lt01 = if n_c == 0 {
        0.0
    } else {
        depths.iter().filter(|&&d| d < 0.01).count() as f64 / n_c as f64
    };

    let mut pairs: HashSet<(usize, usize)> = HashSet::new();
    let mut wait = 0.0;
    let steal = 0.5f64;
    for e in edges {
        if e.class != "program" {
            continue;
        }
        if !pairs.insert((e.producer_tx, e.consumer_tx)) {
            continue;
        }
        let lag = e.consumer_tx.saturating_sub(e.producer_tx) as f64;
        let lag_norm = if n_tx == 0 {
            0.0
        } else {
            (lag / n_tx as f64).min(1.0)
        };
        wait += lag_norm * (1.0 - steal);
    }

    MaMdProxy {
        n_consumers_with_program_cross: n_c,
        ma_redo_cost: ma_redo,
        md_redo_cost: md_redo,
        redo_saved,
        wait_added: wait,
        depth_p10: percentile_f64(&depths, 0.1),
        depth_p50: percentile_f64(&depths, 0.5),
        depth_p90: percentile_f64(&depths, 0.9),
        depth_mean: mean,
        frac_depth_lt_0_01: lt01,
    }
}
