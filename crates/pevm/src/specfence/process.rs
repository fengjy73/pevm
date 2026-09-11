//! Process-level Fence / Unfenced traces (reason codes + per-ℓ timeline).
//!
//! Not a control plane. Used to prove hot-Region Fence coverage:
//! Unfenced-after-Avoid / Unfenced-after-Fence ≈ 0 on the fan-out ℓ.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::DashMap;
use rustc_hash::FxBuildHasher;
use serde::Serialize;

use crate::{MemoryLocationHash, TxIdx};

/// Why an access was Unfenced (or which Fence verb fired).
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessReason {
    BindPublished,
    WaitForWriter,
    WaitForCanary,
    WaitForSerial,
    WaitForPrefix,
    UnfencedIndependence,
    UnfencedCanary,
    UnfencedCold,
    UnfencedInversion,
    UnfencedWriterDone,
    UnfencedPlantTls,
    /// Should be ~0 on a Fenced hot ℓ after Avoid/publish.
    UnfencedAfterAvoid,
}

const REASON_N: usize = 12;

impl ProcessReason {
    fn idx(self) -> usize {
        match self {
            Self::BindPublished => 0,
            Self::WaitForWriter => 1,
            Self::WaitForCanary => 2,
            Self::WaitForSerial => 3,
            Self::WaitForPrefix => 4,
            Self::UnfencedIndependence => 5,
            Self::UnfencedCanary => 6,
            Self::UnfencedCold => 7,
            Self::UnfencedInversion => 8,
            Self::UnfencedWriterDone => 9,
            Self::UnfencedPlantTls => 10,
            Self::UnfencedAfterAvoid => 11,
        }
    }

    fn from_idx(i: usize) -> Option<Self> {
        Some(match i {
            0 => Self::BindPublished,
            1 => Self::WaitForWriter,
            2 => Self::WaitForCanary,
            3 => Self::WaitForSerial,
            4 => Self::WaitForPrefix,
            5 => Self::UnfencedIndependence,
            6 => Self::UnfencedCanary,
            7 => Self::UnfencedCold,
            8 => Self::UnfencedInversion,
            9 => Self::UnfencedWriterDone,
            10 => Self::UnfencedPlantTls,
            11 => Self::UnfencedAfterAvoid,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BindPublished => "bind_published",
            Self::WaitForWriter => "wait_for_writer",
            Self::WaitForCanary => "wait_for_canary",
            Self::WaitForSerial => "wait_for_serial",
            Self::WaitForPrefix => "wait_for_prefix",
            Self::UnfencedIndependence => "unfenced_independence",
            Self::UnfencedCanary => "unfenced_canary",
            Self::UnfencedCold => "unfenced_cold",
            Self::UnfencedInversion => "unfenced_inversion",
            Self::UnfencedWriterDone => "unfenced_writer_done",
            Self::UnfencedPlantTls => "unfenced_plant_tls",
            Self::UnfencedAfterAvoid => "unfenced_after_avoid",
        }
    }

    pub fn is_unfenced(self) -> bool {
        matches!(
            self,
            Self::UnfencedIndependence
                | Self::UnfencedCanary
                | Self::UnfencedCold
                | Self::UnfencedInversion
                | Self::UnfencedWriterDone
                | Self::UnfencedPlantTls
                | Self::UnfencedAfterAvoid
        )
    }
}

#[derive(Debug, Default)]
struct LocProc {
    bind: AtomicUsize,
    wait_for: AtomicUsize,
    unfenced: AtomicUsize,
    unfenced_before_avoid: AtomicUsize,
    unfenced_after_avoid: AtomicUsize,
    wait_after_avoid: AtomicUsize,
    bind_after_avoid: AtomicUsize,
    unfenced_after_canary: AtomicUsize,
    first_avoid_seq: AtomicU64,
    first_canary_seq: AtomicU64,
    first_fence_seq: AtomicU64,
    reasons: [AtomicUsize; REASON_N],
}

/// Live process tracer (one block).
#[derive(Debug, Default)]
pub struct ProcessTrace {
    reasons: [AtomicUsize; REASON_N],
    seq: AtomicU64,
    locs: DashMap<MemoryLocationHash, LocProc, FxBuildHasher>,
}

/// One location's Fence / Unfenced split.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct LocProcessSnap {
    pub location: u64,
    pub bind: usize,
    pub wait_for: usize,
    pub unfenced: usize,
    pub unfenced_before_avoid: usize,
    pub unfenced_after_avoid: usize,
    pub wait_after_avoid: usize,
    pub bind_after_avoid: usize,
    pub unfenced_after_canary: usize,
    pub first_avoid_seq: u64,
    pub first_canary_seq: u64,
    pub first_fence_seq: u64,
    pub reasons: BTreeMap<String, usize>,
}

/// Block-level process snapshot (JSON).
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct ExecProcessSnapshot {
    pub reason_histogram: BTreeMap<String, usize>,
    pub unfenced_total: usize,
    pub wait_for_total: usize,
    pub bind_total: usize,
    pub unfenced_after_avoid_total: usize,
    pub hot_locations: Vec<LocProcessSnap>,
    pub hot_fanout_l: Option<LocProcessSnap>,
    pub unfenced_after_fence_on_hot_l: usize,
    pub independent_unfenced_total: usize,
}

impl ProcessTrace {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(
        &self,
        location: MemoryLocationHash,
        _reader: TxIdx,
        reason: ProcessReason,
        avoid: bool,
        canary_taken: bool,
    ) {
        let i = reason.idx();
        self.reasons[i].fetch_add(1, Ordering::Relaxed);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let e = self.locs.entry(location).or_default();
        e.reasons[i].fetch_add(1, Ordering::Relaxed);
        match reason {
            ProcessReason::BindPublished => {
                e.bind.fetch_add(1, Ordering::Relaxed);
                if e.first_fence_seq.load(Ordering::Relaxed) == 0 {
                    let _ = e.first_fence_seq.compare_exchange(
                        0,
                        seq,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                if avoid {
                    e.bind_after_avoid.fetch_add(1, Ordering::Relaxed);
                }
            }
            ProcessReason::WaitForWriter
            | ProcessReason::WaitForCanary
            | ProcessReason::WaitForSerial
            | ProcessReason::WaitForPrefix => {
                e.wait_for.fetch_add(1, Ordering::Relaxed);
                if e.first_fence_seq.load(Ordering::Relaxed) == 0 {
                    let _ = e.first_fence_seq.compare_exchange(
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
                e.unfenced.fetch_add(1, Ordering::Relaxed);
                if reason == ProcessReason::UnfencedCanary
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
                    e.unfenced_after_avoid.fetch_add(1, Ordering::Relaxed);
                } else {
                    e.unfenced_before_avoid.fetch_add(1, Ordering::Relaxed);
                }
                if canary_taken && reason != ProcessReason::UnfencedCanary {
                    e.unfenced_after_canary.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub(crate) fn note_avoid(&self, location: MemoryLocationHash) {
        let seq = self.seq.load(Ordering::Relaxed);
        let e = self.locs.entry(location).or_default();
        if e.first_avoid_seq.load(Ordering::Relaxed) == 0 {
            let _ = e
                .first_avoid_seq
                .compare_exchange(0, seq.max(1), Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    pub(crate) fn snapshot(&self, top_n: usize) -> ExecProcessSnapshot {
        let mut hist = BTreeMap::new();
        let mut unfenced_total = 0usize;
        let mut wait_for_total = 0usize;
        let mut bind_total = 0usize;
        for i in 0..REASON_N {
            let n = self.reasons[i].load(Ordering::Relaxed);
            if let Some(r) = ProcessReason::from_idx(i) {
                hist.insert(r.as_str().to_string(), n);
                if r.is_unfenced() {
                    unfenced_total += n;
                } else if r == ProcessReason::BindPublished {
                    bind_total += n;
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
                    bind: v.bind.load(Ordering::Relaxed),
                    wait_for: v.wait_for.load(Ordering::Relaxed),
                    unfenced: v.unfenced.load(Ordering::Relaxed),
                    unfenced_before_avoid: v.unfenced_before_avoid.load(Ordering::Relaxed),
                    unfenced_after_avoid: v.unfenced_after_avoid.load(Ordering::Relaxed),
                    wait_after_avoid: v.wait_after_avoid.load(Ordering::Relaxed),
                    bind_after_avoid: v.bind_after_avoid.load(Ordering::Relaxed),
                    unfenced_after_canary: v.unfenced_after_canary.load(Ordering::Relaxed),
                    first_avoid_seq: v.first_avoid_seq.load(Ordering::Relaxed),
                    first_canary_seq: v.first_canary_seq.load(Ordering::Relaxed),
                    first_fence_seq: v.first_fence_seq.load(Ordering::Relaxed),
                    reasons,
                }
            })
            .collect();
        locs.sort_by(|a, b| {
            let sa = a.unfenced + a.wait_for + a.bind;
            let sb = b.unfenced + b.wait_for + b.bind;
            sb.cmp(&sa).then_with(|| {
                (b.unfenced_after_avoid + b.wait_after_avoid + b.bind_after_avoid)
                    .cmp(&(a.unfenced_after_avoid + a.wait_after_avoid + a.bind_after_avoid))
            })
        });
        let unfenced_after_avoid_total = locs.iter().map(|l| l.unfenced_after_avoid).sum();
        let hot_fanout_l = locs.first().cloned();
        let unfenced_after_fence_on_hot_l = hot_fanout_l
            .as_ref()
            .map(|l| l.unfenced_after_avoid + l.unfenced_after_canary)
            .unwrap_or(0);
        let independent_unfenced_total = self.reasons[ProcessReason::UnfencedIndependence.idx()]
            .load(Ordering::Relaxed);
        if locs.len() > top_n {
            locs.truncate(top_n);
        }
        ExecProcessSnapshot {
            reason_histogram: hist,
            unfenced_total,
            wait_for_total,
            bind_total,
            unfenced_after_avoid_total,
            hot_locations: locs,
            hot_fanout_l,
            unfenced_after_fence_on_hot_l,
            independent_unfenced_total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_avoid_unfenced_counted() {
        let t = ProcessTrace::new();
        t.record(7, 3, ProcessReason::UnfencedCanary, false, false);
        t.note_avoid(7);
        t.record(7, 4, ProcessReason::WaitForWriter, true, true);
        t.record(7, 5, ProcessReason::UnfencedAfterAvoid, true, true);
        t.record(7, 6, ProcessReason::BindPublished, true, true);
        let s = t.snapshot(4);
        let hot = s.hot_fanout_l.expect("hot");
        assert_eq!(hot.unfenced_before_avoid, 1);
        assert_eq!(hot.unfenced_after_avoid, 1);
        assert_eq!(hot.wait_after_avoid, 1);
        assert_eq!(hot.bind_after_avoid, 1);
        assert_eq!(s.unfenced_after_avoid_total, 1);
    }
}
