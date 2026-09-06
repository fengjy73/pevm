#![allow(missing_docs)]
//! Fine-grain serial/OCC runtime tracing for lab analysis (opt-in).
//!
//! Disabled by default: zero cost on production paths when `Pevm::finegrain_trace`
//! is off. When enabled, parallel execution snapshots final RW sets (≈ G*),
//! location kinds, per-tx incarnation highs, and OCC abort events.

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
}

/// Opt-in collector living on [`crate::Pevm`].
#[derive(Debug, Default)]
pub struct FineGrainCollector {
    abort_events: Mutex<Vec<AbortEvent>>,
    last_snapshot: Mutex<Option<FineGrainSnapshot>>,
}

impl FineGrainCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        self.abort_events.lock().unwrap().clear();
        *self.last_snapshot.lock().unwrap() = None;
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
        *self.last_snapshot.lock().unwrap() = Some(FineGrainSnapshot {
            n_tx,
            beneficiary_hash,
            txs,
            location_kinds,
            abort_events,
            final_incarnations,
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
