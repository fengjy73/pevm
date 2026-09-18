//! Process-level pessimistic-admit / optimistic-read traces (reason codes + per-ℓ timeline).
//!
//! Not a control plane. Used to prove hot-Region pessimistic-admit coverage:
//! optimistic_read after Avoid / after pessimistic admit ≈ 0 on the fan-out ℓ.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::DashMap;
use rustc_hash::FxBuildHasher;
use serde::Serialize;

use crate::{MemoryLocationHash, TxIdx};

use super::decision_field::{DecisionFeat, DecisionFieldAgg, DecisionFieldSnap, DecisionVerb};

/// Why an access was OptimisticRead (or which Fence verb fired).
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessReason {
    OrderedAdmitPublished,
    WaitForWriter,
    WaitForCanary,
    WaitForSerial,
    WaitForPrefix,
    OptimisticReadIndependence,
    OptimisticReadCanary,
    OptimisticReadCold,
    OptimisticReadInversion,
    OptimisticReadWriterDone,
    OptimisticReadProtocolTls,
    /// Should be ~0 on a Fenced hot ℓ after Avoid/publish.
    OptimisticReadAfterAvoid,
}

const REASON_N: usize = 12;

impl ProcessReason {
    fn idx(self) -> usize {
        match self {
            Self::OrderedAdmitPublished => 0,
            Self::WaitForWriter => 1,
            Self::WaitForCanary => 2,
            Self::WaitForSerial => 3,
            Self::WaitForPrefix => 4,
            Self::OptimisticReadIndependence => 5,
            Self::OptimisticReadCanary => 6,
            Self::OptimisticReadCold => 7,
            Self::OptimisticReadInversion => 8,
            Self::OptimisticReadWriterDone => 9,
            Self::OptimisticReadProtocolTls => 10,
            Self::OptimisticReadAfterAvoid => 11,
        }
    }

    fn from_idx(i: usize) -> Option<Self> {
        Some(match i {
            0 => Self::OrderedAdmitPublished,
            1 => Self::WaitForWriter,
            2 => Self::WaitForCanary,
            3 => Self::WaitForSerial,
            4 => Self::WaitForPrefix,
            5 => Self::OptimisticReadIndependence,
            6 => Self::OptimisticReadCanary,
            7 => Self::OptimisticReadCold,
            8 => Self::OptimisticReadInversion,
            9 => Self::OptimisticReadWriterDone,
            10 => Self::OptimisticReadProtocolTls,
            11 => Self::OptimisticReadAfterAvoid,
            _ => return None,
        })
    }

    /// Stable JSON key for this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OrderedAdmitPublished => "ordered_admit_published",
            Self::WaitForWriter => "wait_for_writer",
            Self::WaitForCanary => "wait_for_canary",
            Self::WaitForSerial => "wait_for_serial",
            Self::WaitForPrefix => "wait_for_prefix",
            Self::OptimisticReadIndependence => "optimistic_read_independence",
            Self::OptimisticReadCanary => "optimistic_read_canary",
            Self::OptimisticReadCold => "optimistic_read_cold",
            Self::OptimisticReadInversion => "optimistic_read_inversion",
            Self::OptimisticReadWriterDone => "optimistic_read_writer_done",
            Self::OptimisticReadProtocolTls => "optimistic_read_protocol_tls",
            Self::OptimisticReadAfterAvoid => "optimistic_read_after_avoid",
        }
    }

    /// True when this verb is OptimisticRead (not a Fence).
    pub fn is_optimistic_read(self) -> bool {
        matches!(
            self,
            Self::OptimisticReadIndependence
                | Self::OptimisticReadCanary
                | Self::OptimisticReadCold
                | Self::OptimisticReadInversion
                | Self::OptimisticReadWriterDone
                | Self::OptimisticReadProtocolTls
                | Self::OptimisticReadAfterAvoid
        )
    }
}

#[derive(Debug, Default)]
struct LocProc {
    ordered_admit: AtomicUsize,
    wait_for: AtomicUsize,
    optimistic_read: AtomicUsize,
    optimistic_read_before_avoid: AtomicUsize,
    optimistic_read_after_avoid: AtomicUsize,
    wait_after_avoid: AtomicUsize,
    ordered_admit_after_avoid: AtomicUsize,
    optimistic_read_after_canary: AtomicUsize,
    first_avoid_seq: AtomicU64,
    first_canary_seq: AtomicU64,
    first_pessimistic_admit_seq: AtomicU64,
    reasons: [AtomicUsize; REASON_N],
}

#[derive(Debug, Default)]
struct PerTxProc {
    ordered_admit: AtomicUsize,
    wait_for: AtomicUsize,
    optimistic_read: AtomicUsize,
    optimistic_read_after_avoid: AtomicUsize,
    force_prefix_none: AtomicUsize,
    n_park: AtomicUsize,
    reasons: [AtomicUsize; REASON_N],
}

/// Live process tracer (one block).
#[derive(Debug, Default)]
pub(crate) struct ProcessTrace {
    reasons: [AtomicUsize; REASON_N],
    seq: AtomicU64,
    locs: DashMap<MemoryLocationHash, LocProc, FxBuildHasher>,
    txs: DashMap<TxIdx, PerTxProc, FxBuildHasher>,
    force_prefix_none_optimistic_read: AtomicUsize,
    /// Temporary lab: feature × verb contingencies for π field selection.
    decision_fields: DecisionFieldAgg,
}

/// One location's Fence / OptimisticRead split.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct LocProcessSnap {
    pub location: u64,
    pub ordered_admit: usize,
    pub wait_for: usize,
    pub optimistic_read: usize,
    pub optimistic_read_before_avoid: usize,
    pub optimistic_read_after_avoid: usize,
    pub wait_after_avoid: usize,
    pub ordered_admit_after_avoid: usize,
    pub optimistic_read_after_canary: usize,
    pub first_avoid_seq: u64,
    pub first_canary_seq: u64,
    pub first_pessimistic_admit_seq: u64,
    pub reasons: BTreeMap<String, usize>,
}

/// Per-tx Fence / OptimisticRead / park split (diagnosis).
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct PerTxProcessSnap {
    pub tx: usize,
    pub n_ordered_admit: usize,
    pub n_wait_for: usize,
    pub n_optimistic_read: usize,
    pub n_optimistic_read_after_avoid: usize,
    pub n_force_prefix_none: usize,
    pub n_park: usize,
    pub reasons: BTreeMap<String, usize>,
}

/// Block-level process snapshot (JSON).
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct ExecProcessSnapshot {
    pub reason_histogram: BTreeMap<String, usize>,
    pub optimistic_read_total: usize,
    pub wait_for_total: usize,
    pub ordered_admit_total: usize,
    pub optimistic_read_after_avoid_total: usize,
    pub hot_locations: Vec<LocProcessSnap>,
    pub hot_fanout_l: Option<LocProcessSnap>,
    pub optimistic_read_after_pessimistic_admit_on_hot_l: usize,
    pub independent_optimistic_read_total: usize,
    /// U1 leak counter: force_prefix ∧ writer unresolved → OptimisticRead (target 0).
    pub force_prefix_none_optimistic_read: usize,
    /// Per-reader process verbs (sorted by tx).
    pub per_tx: Vec<PerTxProcessSnap>,
    /// Temporary lab: decision-field contingencies.
    pub decision_fields: DecisionFieldSnap,
    /// Frozen-grain: txs that mixed Fence + OptimisticRead (expected >0 on fan_out).
    pub mixed_verb_intra_tx: usize,
}

impl ProcessTrace {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// End-tx OptimisticRead flush (no per-SLOAD DashMap). Increments cold OptimisticRead
    /// so `mixed_verb_intra_tx` still sees OrderedAdmit+OptimisticRead in one reader.
    pub(crate) fn note_optimistic_read_occ(&self, reader: TxIdx, n: u32) {
        if n == 0 {
            return;
        }
        let n = n as usize;
        self.reasons[ProcessReason::OptimisticReadCold.idx()].fetch_add(n, Ordering::Relaxed);
        let txe = self.txs.entry(reader).or_default();
        txe.optimistic_read.fetch_add(n, Ordering::Relaxed);
        txe.reasons[ProcessReason::OptimisticReadCold.idx()].fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn record(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
        reason: ProcessReason,
        avoid: bool,
        canary_taken: bool,
    ) {
        let i = reason.idx();
        self.reasons[i].fetch_add(1, Ordering::Relaxed);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let txe = self.txs.entry(reader).or_default();
        txe.reasons[i].fetch_add(1, Ordering::Relaxed);
        match reason {
            ProcessReason::OrderedAdmitPublished => {
                txe.ordered_admit.fetch_add(1, Ordering::Relaxed);
            }
            ProcessReason::WaitForWriter
            | ProcessReason::WaitForCanary
            | ProcessReason::WaitForSerial
            | ProcessReason::WaitForPrefix => {
                txe.wait_for.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                txe.optimistic_read.fetch_add(1, Ordering::Relaxed);
                if avoid {
                    txe.optimistic_read_after_avoid
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        drop(txe);
        let e = self.locs.entry(location).or_default();
        e.reasons[i].fetch_add(1, Ordering::Relaxed);
        match reason {
            ProcessReason::OrderedAdmitPublished => {
                e.ordered_admit.fetch_add(1, Ordering::Relaxed);
                if e.first_pessimistic_admit_seq.load(Ordering::Relaxed) == 0 {
                    let _ = e.first_pessimistic_admit_seq.compare_exchange(
                        0,
                        seq,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                if avoid {
                    e.ordered_admit_after_avoid.fetch_add(1, Ordering::Relaxed);
                }
            }
            ProcessReason::WaitForWriter
            | ProcessReason::WaitForCanary
            | ProcessReason::WaitForSerial
            | ProcessReason::WaitForPrefix => {
                e.wait_for.fetch_add(1, Ordering::Relaxed);
                if e.first_pessimistic_admit_seq.load(Ordering::Relaxed) == 0 {
                    let _ = e.first_pessimistic_admit_seq.compare_exchange(
                        0,
                        seq,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                if avoid {
                    e.wait_after_avoid.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {
                e.optimistic_read.fetch_add(1, Ordering::Relaxed);
                if reason == ProcessReason::OptimisticReadCanary
                    && e.first_canary_seq.load(Ordering::Relaxed) == 0
                {
                    let _ = e.first_canary_seq.compare_exchange(
                        0,
                        seq,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                if avoid {
                    e.optimistic_read_after_avoid
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    e.optimistic_read_before_avoid
                        .fetch_add(1, Ordering::Relaxed);
                }
                if canary_taken && reason != ProcessReason::OptimisticReadCanary {
                    e.optimistic_read_after_canary
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub(crate) fn note_force_prefix_none_optimistic_read(&self, reader: TxIdx) {
        self.force_prefix_none_optimistic_read
            .fetch_add(1, Ordering::Relaxed);
        self.txs
            .entry(reader)
            .or_default()
            .force_prefix_none
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_park(&self, reader: TxIdx) {
        self.txs
            .entry(reader)
            .or_default()
            .n_park
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_avoid(&self, location: MemoryLocationHash) {
        let seq = self.seq.load(Ordering::Relaxed);
        let e = self.locs.entry(location).or_default();
        if e.first_avoid_seq.load(Ordering::Relaxed) == 0 {
            let _ = e.first_avoid_seq.compare_exchange(
                0,
                seq.max(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    pub(crate) fn record_decision(&self, feat: DecisionFeat) {
        self.decision_fields.record(feat);
    }

    pub(crate) fn decision_fields_snapshot(&self) -> DecisionFieldSnap {
        self.decision_fields.snapshot()
    }

    pub(crate) fn snapshot(&self, top_n: usize) -> ExecProcessSnapshot {
        let mut hist = BTreeMap::new();
        let mut optimistic_read_total = 0usize;
        let mut wait_for_total = 0usize;
        let mut ordered_admit_total = 0usize;
        for i in 0..REASON_N {
            let n = self.reasons[i].load(Ordering::Relaxed);
            if let Some(r) = ProcessReason::from_idx(i) {
                hist.insert(r.as_str().to_string(), n);
                if r.is_optimistic_read() {
                    optimistic_read_total += n;
                } else if r == ProcessReason::OrderedAdmitPublished {
                    ordered_admit_total += n;
                } else {
                    wait_for_total += n;
                }
            }
        }
        let mut locs: Vec<LocProcessSnap> = self
            .locs
            .iter()
            .map(|e| {
                let loc = *e.key();
                let v = e.value();
                let mut reasons = BTreeMap::new();
                for i in 0..REASON_N {
                    let n = v.reasons[i].load(Ordering::Relaxed);
                    if n > 0 {
                        if let Some(r) = ProcessReason::from_idx(i) {
                            reasons.insert(r.as_str().to_string(), n);
                        }
                    }
                }
                LocProcessSnap {
                    location: loc as u64,
                    ordered_admit: v.ordered_admit.load(Ordering::Relaxed),
                    wait_for: v.wait_for.load(Ordering::Relaxed),
                    optimistic_read: v.optimistic_read.load(Ordering::Relaxed),
                    optimistic_read_before_avoid: v
                        .optimistic_read_before_avoid
                        .load(Ordering::Relaxed),
                    optimistic_read_after_avoid: v
                        .optimistic_read_after_avoid
                        .load(Ordering::Relaxed),
                    wait_after_avoid: v.wait_after_avoid.load(Ordering::Relaxed),
                    ordered_admit_after_avoid: v.ordered_admit_after_avoid.load(Ordering::Relaxed),
                    optimistic_read_after_canary: v
                        .optimistic_read_after_canary
                        .load(Ordering::Relaxed),
                    first_avoid_seq: v.first_avoid_seq.load(Ordering::Relaxed),
                    first_canary_seq: v.first_canary_seq.load(Ordering::Relaxed),
                    first_pessimistic_admit_seq: v
                        .first_pessimistic_admit_seq
                        .load(Ordering::Relaxed),
                    reasons,
                }
            })
            .collect();
        // Fan-out clique = max post-Avoid OrderedAdmit/Wait, not max raw OptimisticRead
        // (independence chatter can out-count the star ℓ).
        locs.sort_by(|a, b| {
            let sa = a.ordered_admit_after_avoid + a.wait_after_avoid;
            let sb = b.ordered_admit_after_avoid + b.wait_after_avoid;
            sb.cmp(&sa).then_with(|| {
                (b.ordered_admit + b.wait_for)
                    .cmp(&(a.ordered_admit + a.wait_for))
                    .then_with(|| {
                        (b.optimistic_read + b.wait_for + b.ordered_admit)
                            .cmp(&(a.optimistic_read + a.wait_for + a.ordered_admit))
                    })
            })
        });
        let optimistic_read_after_avoid_total =
            locs.iter().map(|l| l.optimistic_read_after_avoid).sum();
        let hot_fanout_l = locs.first().cloned();
        let optimistic_read_after_pessimistic_admit_on_hot_l = hot_fanout_l
            .as_ref()
            .map(|l| l.optimistic_read_after_avoid)
            .unwrap_or(0);
        let independent_optimistic_read_total =
            self.reasons[ProcessReason::OptimisticReadIndependence.idx()].load(Ordering::Relaxed);
        if locs.len() > top_n {
            locs.truncate(top_n);
        }
        let mut per_tx: Vec<PerTxProcessSnap> = self
            .txs
            .iter()
            .map(|e| {
                let tx = *e.key();
                let v = e.value();
                let mut reasons = BTreeMap::new();
                for i in 0..REASON_N {
                    let n = v.reasons[i].load(Ordering::Relaxed);
                    if n > 0 {
                        if let Some(r) = ProcessReason::from_idx(i) {
                            reasons.insert(r.as_str().to_string(), n);
                        }
                    }
                }
                PerTxProcessSnap {
                    tx,
                    n_ordered_admit: v.ordered_admit.load(Ordering::Relaxed),
                    n_wait_for: v.wait_for.load(Ordering::Relaxed),
                    n_optimistic_read: v.optimistic_read.load(Ordering::Relaxed),
                    n_optimistic_read_after_avoid: v
                        .optimistic_read_after_avoid
                        .load(Ordering::Relaxed),
                    n_force_prefix_none: v.force_prefix_none.load(Ordering::Relaxed),
                    n_park: v.n_park.load(Ordering::Relaxed),
                    reasons,
                }
            })
            .collect();
        per_tx.sort_by_key(|t| t.tx);
        let mixed_verb_intra_tx = per_tx
            .iter()
            .filter(|t| (t.n_ordered_admit + t.n_wait_for) > 0 && t.n_optimistic_read > 0)
            .count();
        ExecProcessSnapshot {
            reason_histogram: hist,
            optimistic_read_total,
            wait_for_total,
            ordered_admit_total,
            optimistic_read_after_avoid_total,
            hot_locations: locs,
            hot_fanout_l,
            optimistic_read_after_pessimistic_admit_on_hot_l,
            independent_optimistic_read_total,
            force_prefix_none_optimistic_read: self
                .force_prefix_none_optimistic_read
                .load(Ordering::Relaxed),
            per_tx,
            decision_fields: self.decision_fields.snapshot(),
            mixed_verb_intra_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_avoid_optimistic_read_counted() {
        let t = ProcessTrace::new();
        t.record(7, 3, ProcessReason::OptimisticReadCanary, false, false);
        t.note_avoid(7);
        t.record(7, 4, ProcessReason::WaitForWriter, true, true);
        t.record(7, 5, ProcessReason::OptimisticReadAfterAvoid, true, true);
        t.record(7, 6, ProcessReason::OrderedAdmitPublished, true, true);
        let s = t.snapshot(4);
        let hot = s.hot_fanout_l.expect("hot");
        assert_eq!(hot.optimistic_read_before_avoid, 1);
        assert_eq!(hot.optimistic_read_after_avoid, 1);
        assert_eq!(hot.wait_after_avoid, 1);
        assert_eq!(hot.ordered_admit_after_avoid, 1);
        assert_eq!(s.optimistic_read_after_avoid_total, 1);
    }
}
