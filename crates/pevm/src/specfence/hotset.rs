//! Adaptive CC Redesign v1 — location-local HotSet (R1).
//!
//! A location enters HotSet when any holds (online, revocable):
//! 1. Observed ≥ H_w writers in-block (default 8), or
//! 2. Abort/ESTIMATE involving ℓ ≥ H_a times (default 3), or
//! 3. Process prior: sustained multi-writer mass for ℓ above threshold
//!    (needs repeated reinforcement; decays when unseen so wide blocks stay lean).
//!
//! WaitHard / Bayes Wait are forbidden for ℓ ∉ HotSet (LeanOCC SpecRead only).

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use hashbrown::HashSet;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

/// In-block distinct writers before ℓ is hot.
/// R3: raised 3→8 so wide blocks (max_writers≈5–7 on G*) keep hotset≈0;
/// true hot chains (tens–hundreds of writers) still escalate.
pub(crate) const H_W: usize = 8;
/// Abort/ESTIMATE events on ℓ before it is hot.
/// R3: raised 2→3 to cut abort-noise inserts on wide DAG.
pub(crate) const H_A: usize = 3;
/// Process prior multi-writer mass that seeds HotSet at block start.
/// R3: raised so a single block's or_insert(α) cannot seed the next wide block.
const PROCESS_PRIOR_THRESHOLD: f64 = 0.80;
/// EWMA α for process multi-writer mass (first sighting inserts α, not 1.0).
const PRIOR_ALPHA: f64 = 0.6;
/// Per-block decay for priors not reinforced this block.
const PRIOR_UNSEEN_DECAY: f64 = 0.5;
/// Drop priors below this after decay.
const PRIOR_EVICT_FLOOR: f64 = 0.05;
const MAX_PRIOR_ENTRIES: usize = 8192;

#[derive(Debug, Default)]
struct LocWriters {
    txs: HashSet<TxIdx>,
}

/// Per-block HotSet + process-persistent multi-writer prior.
#[derive(Debug)]
pub(crate) struct HotSet {
    /// Locations currently treated as hot (this block).
    members: DashMap<MemoryLocationHash, (), BuildIdentityHasher>,
    /// Distinct writers observed so far this block.
    writers: DashMap<MemoryLocationHash, LocWriters, BuildIdentityHasher>,
    /// Abort/ESTIMATE counts this block.
    aborts: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Process-persistent multi-writer mass (inter-block).
    process_prior: DashMap<MemoryLocationHash, f64, BuildIdentityHasher>,
    /// Diagnostics: HotLocal resolve invocations.
    hot_local_reads: AtomicUsize,
}

impl Default for HotSet {
    fn default() -> Self {
        Self::new()
    }
}

impl HotSet {
    pub(crate) fn new() -> Self {
        Self {
            members: DashMap::default(),
            writers: DashMap::default(),
            aborts: DashMap::default(),
            process_prior: DashMap::default(),
            hot_local_reads: AtomicUsize::new(0),
        }
    }

    /// Clear per-block state; keep process prior.
    /// R3: do **not** pre-seed members from process prior at block start — that
    /// polluted wide blocks after a hot block in the same process (shared token
    /// location hashes). Prior still accelerates via `note_writer` sticky once a
    /// write is observed on ℓ this block.
    pub(crate) fn begin_block(&self) {
        self.members.clear();
        self.writers.clear();
        self.aborts.clear();
        self.hot_local_reads.store(0, Ordering::Relaxed);
    }

    /// Full reset (tests / replay).
    pub(crate) fn reset(&self) {
        self.members.clear();
        self.writers.clear();
        self.aborts.clear();
        self.process_prior.clear();
        self.hot_local_reads.store(0, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn contains(&self, location: MemoryLocationHash) -> bool {
        self.members.contains_key(&location)
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.members.len()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Record that `tx_idx` wrote `location`. Inserts HotSet at ≥ H_w writers.
    pub(crate) fn note_writer(&self, location: MemoryLocationHash, tx_idx: TxIdx) -> bool {
        let mut inserted = false;
        {
            let mut entry = self.writers.entry(location).or_default();
            entry.txs.insert(tx_idx);
            if entry.txs.len() >= H_W {
                inserted = self.members.insert(location, ()).is_none();
            }
        }
        // Strong process prior also keeps membership sticky within the block.
        if !self.contains(location)
            && self
                .process_prior
                .get(&location)
                .is_some_and(|m| *m >= PROCESS_PRIOR_THRESHOLD)
        {
            inserted = self.members.insert(location, ()).is_none() || inserted;
        }
        inserted
    }

    /// Record abort/ESTIMATE involving `location`. Inserts HotSet at ≥ H_a.
    pub(crate) fn note_abort(&self, location: MemoryLocationHash) -> bool {
        let count = {
            let entry = self
                .aborts
                .entry(location)
                .or_insert_with(|| AtomicUsize::new(0));
            entry.fetch_add(1, Ordering::Relaxed) + 1
        };
        if count >= H_A {
            self.members.insert(location, ()).is_none()
        } else {
            false
        }
    }

    /// Force-insert (e.g. escalate writers' locs when abort_rate ≥ 0.08).
    pub(crate) fn insert(&self, location: MemoryLocationHash) {
        self.members.insert(location, ());
    }

    pub(crate) fn record_hot_local_read(&self) {
        self.hot_local_reads.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn hot_local_reads(&self) -> usize {
        self.hot_local_reads.load(Ordering::Relaxed)
    }

    /// End-of-block: reinforce multi-writer / abort-storm locs; decay the rest.
    pub(crate) fn end_block(&self) {
        let mut reinforced = HashSet::<MemoryLocationHash>::new();
        for entry in self.writers.iter() {
            if entry.value().txs.len() >= H_W {
                let loc = *entry.key();
                reinforced.insert(loc);
                self.process_prior
                    .entry(loc)
                    .and_modify(|m| *m = PRIOR_ALPHA + (1.0 - PRIOR_ALPHA) * *m)
                    .or_insert(PRIOR_ALPHA);
            }
        }
        // Also reinforce locations that entered via abort storms.
        for entry in self.members.iter() {
            let loc = *entry.key();
            if self
                .aborts
                .get(&loc)
                .is_some_and(|c| c.load(Ordering::Relaxed) >= H_A)
            {
                reinforced.insert(loc);
                self.process_prior
                    .entry(loc)
                    .and_modify(|m| *m = PRIOR_ALPHA + (1.0 - PRIOR_ALPHA) * *m)
                    .or_insert(PRIOR_ALPHA);
            }
        }
        // Decay priors not reinforced this block so one-shot heat cannot seed forever.
        let all_keys: Vec<MemoryLocationHash> =
            self.process_prior.iter().map(|e| *e.key()).collect();
        for loc in all_keys {
            if reinforced.contains(&loc) {
                continue;
            }
            let mut drop = false;
            if let Some(mut m) = self.process_prior.get_mut(&loc) {
                *m *= PRIOR_UNSEEN_DECAY;
                if *m < PRIOR_EVICT_FLOOR {
                    drop = true;
                }
            }
            if drop {
                self.process_prior.remove(&loc);
            }
        }
        self.evict_prior_if_needed();
    }

    fn evict_prior_if_needed(&self) {
        let extra = self.process_prior.len().saturating_sub(MAX_PRIOR_ENTRIES);
        if extra == 0 {
            return;
        }
        let mut coldest: Vec<(MemoryLocationHash, f64)> = self
            .process_prior
            .iter()
            .map(|e| (*e.key(), *e.value()))
            .collect();
        coldest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (loc, _) in coldest.into_iter().take(extra) {
            self.process_prior.remove(&loc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers_insert_at_h_w() {
        let hs = HotSet::new();
        for i in 0..(H_W - 1) {
            assert!(!hs.note_writer(42, i));
        }
        assert!(hs.note_writer(42, H_W - 1)); // H_w-th distinct writer
        assert!(hs.contains(42));
        assert_eq!(hs.len(), 1);
    }

    #[test]
    fn abort_insert_at_h_a() {
        let hs = HotSet::new();
        for _ in 0..(H_A - 1) {
            assert!(!hs.note_abort(7));
        }
        assert!(hs.note_abort(7));
        assert!(hs.contains(7));
    }

    #[test]
    fn process_prior_sticky_on_writer_after_sustained_heat() {
        let hs = HotSet::new();
        // One hot block: prior=α < threshold — no sticky on first write next block.
        for i in 0..H_W {
            hs.note_writer(99, i);
        }
        hs.end_block();
        hs.begin_block();
        assert!(!hs.contains(99), "begin_block must not pre-seed");
        assert!(
            !hs.note_writer(99, 0),
            "weak prior must not sticky-insert on first write"
        );
        // Second consecutive hot block reinforces above threshold.
        for i in 1..H_W {
            hs.note_writer(99, i);
        }
        hs.end_block();
        hs.begin_block();
        assert!(!hs.contains(99), "still no pre-seed at begin");
        // Strong prior + one write → sticky insert (before H_w).
        assert!(
            hs.note_writer(99, 0),
            "sustained prior should sticky-insert on first write"
        );
        assert!(hs.contains(99));
    }

    #[test]
    fn process_prior_decays_when_unseen() {
        let hs = HotSet::new();
        for _ in 0..3 {
            for i in 0..H_W {
                hs.note_writer(77, i);
            }
            hs.end_block();
            hs.begin_block();
        }
        // Prior strong: one write stickies.
        assert!(hs.note_writer(77, 0));
        assert!(hs.contains(77));
        hs.begin_block();
        // Several cold blocks: decay below threshold.
        for _ in 0..6 {
            hs.end_block();
            hs.begin_block();
        }
        assert!(
            !hs.note_writer(77, 0),
            "decayed prior must not sticky-insert"
        );
    }
}
