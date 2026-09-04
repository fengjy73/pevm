//! Region Execution Machine (REM) plant scaffolding for SpecFence Spec v1.
//!
//! Phase-1 still drives one interpreter session per incarnation (`RunTx`), but
//! must emit region events and expose per-location validate semantics.
//!
//! P2: semantic PartialRetry — revm re-executes from start, but π forces
//! Bind/WaitHard on the previously certified-prefix locations, and only
//! failed-suffix writes are selectively invalidated (no global aborted stamp).

#![allow(dead_code)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use hashbrown::{HashMap, HashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx, TxIncarnation};

/// Spec v1 REM task kinds (plant vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemTask {
    RunTx(TxIdx),
    PublishWrite {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
        incarnation: TxIncarnation,
    },
    ValidateLocation {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    },
    Repair {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    },
    FinalizeTx(TxIdx),
}

/// Access mode of a region effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessMode {
    Read,
    Write,
}

/// One world-state journal effect during `RunTx(t)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegionAccess {
    pub tx_idx: TxIdx,
    /// Monotonic effect ordinal `k` inside the current incarnation.
    pub k: usize,
    pub location: MemoryLocationHash,
    pub mode: AccessMode,
}

/// Decision after classifying a validation failure for PartialRetry.
#[derive(Debug, Clone)]
pub(crate) struct PartialRetryPlan {
    /// Locations whose origins still matched (certified prefix).
    pub certified: Vec<MemoryLocationHash>,
    /// First failed-read effect ordinal.
    pub k_fail: usize,
    /// Write locations to ESTIMATE (failed suffix).
    pub suffix_writes: Vec<MemoryLocationHash>,
    /// Write locations left intact (prefix; no global aborted stamp).
    pub prefix_writes: Vec<MemoryLocationHash>,
}

/// Per-tx checkpoint / certified-prefix state for semantic PartialRetry.
#[derive(Debug, Default)]
pub(crate) struct PartialRetryState {
    /// Monotonic effect ordinal for the current incarnation.
    k: usize,
    /// First-touch effect ordinal per location this incarnation.
    first_k: HashMap<MemoryLocationHash, usize, BuildIdentityHasher>,
    /// Locations that passed EarlyVal (or were otherwise certified) this incarnation.
    certified: HashSet<MemoryLocationHash, BuildIdentityHasher>,
    /// Full access journal (metrics / debugging).
    journal: Vec<RegionAccess>,
}

impl PartialRetryState {
    pub(crate) fn reset(&mut self) {
        self.k = 0;
        self.first_k.clear();
        self.certified.clear();
        self.journal.clear();
    }

    pub(crate) fn note_access(
        &mut self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        mode: AccessMode,
    ) -> usize {
        self.k += 1;
        let k = self.k;
        self.first_k.entry(location).or_insert(k);
        self.journal.push(RegionAccess {
            tx_idx,
            k,
            location,
            mode,
        });
        k
    }

    pub(crate) fn note_certified(&mut self, location: MemoryLocationHash) {
        self.certified.insert(location);
    }

    pub(crate) fn first_k(&self, location: MemoryLocationHash) -> Option<usize> {
        self.first_k.get(&location).copied()
    }

    pub(crate) fn certified_locations(&self) -> Vec<MemoryLocationHash> {
        self.certified.iter().copied().collect()
    }

    pub(crate) fn current_k(&self) -> usize {
        self.k
    }
}

/// Block-scoped PartialRetry plant: per-tx journals + next-incarnation force-bind.
#[derive(Debug)]
pub(crate) struct PartialRetryTable {
    states: Vec<Mutex<PartialRetryState>>,
    /// Locations π must Bind/WaitHard on the next incarnation of `t`.
    force_bind: DashMap<TxIdx, Vec<MemoryLocationHash>, BuildIdentityHasher>,
}

impl PartialRetryTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            states: (0..block_size)
                .map(|_| Mutex::new(PartialRetryState::default()))
                .collect(),
            force_bind: DashMap::default(),
        }
    }

    pub(crate) fn reset_incarnation(&self, tx_idx: TxIdx) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().reset();
        }
    }

    pub(crate) fn note_access(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        mode: AccessMode,
    ) -> usize {
        let mut st = self.states[tx_idx].lock().unwrap();
        st.note_access(tx_idx, location, mode)
    }

    pub(crate) fn note_certified(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        self.states[tx_idx]
            .lock()
            .unwrap()
            .note_certified(location);
    }

    pub(crate) fn first_k(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> Option<usize> {
        self.states[tx_idx].lock().unwrap().first_k(location)
    }

    /// Locations π should force Bind/WaitHard for this incarnation (from prior PartialRetry).
    pub(crate) fn force_bind_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.force_bind
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub(crate) fn must_force_bind(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        self.force_bind
            .get(&tx_idx)
            .is_some_and(|v| v.iter().any(|l| *l == location))
    }

    pub(crate) fn set_force_bind(&self, tx_idx: TxIdx, locations: Vec<MemoryLocationHash>) {
        if locations.is_empty() {
            self.force_bind.remove(&tx_idx);
        } else {
            self.force_bind.insert(tx_idx, locations);
        }
    }

    pub(crate) fn clear_force_bind(&self, tx_idx: TxIdx) {
        self.force_bind.remove(&tx_idx);
    }

    /// Classify validation failure into PartialRetry plan, or `None` if unsafe → FullRetry.
    ///
    /// Safe when there is a non-empty certified-prefix of still-valid reads and we can
    /// split writes: prefix writes ⊆ certified (same location was a certified read before
    /// `k_fail`); everything else is failed-suffix.
    pub(crate) fn plan_partial_retry(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> Option<PartialRetryPlan> {
        if invalid.is_empty() || read_locations.is_empty() {
            return None;
        }
        let st = self.states[tx_idx].lock().unwrap();
        let invalid_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            invalid.iter().copied().collect();
        let mut certified: Vec<MemoryLocationHash> = read_locations
            .iter()
            .copied()
            .filter(|l| !invalid_set.contains(l))
            .collect();
        // Merge EarlyVal certifications that still validate.
        for loc in st.certified.iter() {
            if !invalid_set.contains(loc) && !certified.contains(loc) {
                certified.push(*loc);
            }
        }
        if certified.is_empty() {
            return None;
        }

        let mut k_fail = usize::MAX;
        for &loc in invalid {
            if let Some(k) = st.first_k(loc) {
                k_fail = k_fail.min(k);
            }
        }
        if k_fail == usize::MAX {
            // No journal (shouldn't happen under SpecFence) → unsafe.
            return None;
        }

        let certified_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            certified.iter().copied().collect();

        let mut suffix_writes = Vec::new();
        let mut prefix_writes = Vec::new();
        for &w in write_locations {
            let wk = st.first_k(w).unwrap_or(usize::MAX);
            // Prefix-safe only if touched before k_fail AND location was certified.
            if wk < k_fail && certified_set.contains(&w) {
                prefix_writes.push(w);
            } else {
                suffix_writes.push(w);
            }
        }

        Some(PartialRetryPlan {
            certified,
            k_fail,
            suffix_writes,
            prefix_writes,
        })
    }
}

/// Per-block REM counters (Phase-2 checkpoint prep).
#[derive(Debug, Default)]
pub(crate) struct RemCounters {
    /// Effects observed this block (sum of `k` advances).
    pub effects: AtomicUsize,
    /// Times a checkpoint opportunity was recorded (every successful
    /// per-location validate or every `K` effects).
    pub checkpoint_opportunities: AtomicUsize,
}

impl RemCounters {
    pub(crate) fn note_effect(&self) -> usize {
        self.effects.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn note_checkpoint_opportunity(&self) {
        self.checkpoint_opportunities
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn checkpoint_opportunities(&self) -> usize {
        self.checkpoint_opportunities.load(Ordering::Relaxed)
    }
}

/// Per-worker effect ordinal for the current incarnation.
#[derive(Debug, Default)]
pub(crate) struct EffectOrdinal {
    k: usize,
}

impl EffectOrdinal {
    pub(crate) fn reset(&mut self) {
        self.k = 0;
    }

    pub(crate) fn next(&mut self) -> usize {
        self.k += 1;
        self.k
    }

    pub(crate) fn current(&self) -> usize {
        self.k
    }
}
