//! Fine-grained Detect (D5) + `choose_edge_action` (A1–A4, D6).
//!
//! Conflict identity is **not** a flat `(ℓ, reader)`:
//! - `L_record` = location hash
//! - `L_access` = `(reader, k, depth)` (multi-touch / frames)
//! - `L_edge` = typed wr/rw/ww with unpublished | published-uncommitted | validated
//!
//! π is this module — not AEC EV Await, not Storm/Quiet morph, not OCC-retry.

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use rustc_hash::FxBuildHasher;

use crate::{MemoryLocationHash, TxIdx, TxVersion};

/// Typed anti-dependency (preset-order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EdgeKind {
    /// Writer-then-reader (RAW).
    Wr,
    /// Reader-then-writer (WAR) — rare under preset order; recorded for Detect.
    Rw,
    /// Writer-then-writer (WAW).
    Ww,
}

/// Version visibility on an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeState {
    Unpublished,
    PublishedUncommitted,
    Validated,
}

/// L_access + L_record. Distinct from flatten `(ℓ, reader)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EdgeKey {
    pub location: MemoryLocationHash,
    pub reader: TxIdx,
    pub access_k: u32,
    pub depth: u8,
}

/// One typed edge on an access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EdgeRec {
    pub writer: Option<TxIdx>,
    pub kind: EdgeKind,
    pub state: EdgeState,
}

/// Protocol action — verbs, not EV scores.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EdgeAction {
    /// Install a published version (A3). Writer need not be Validated.
    Bind(TxVersion),
    /// Ordered wait-for an unpublished essential anti-dep (D6).
    WaitFor(TxIdx),
    /// Independence-certified or first-wave canary discovery.
    SpecRead,
}

/// Features for `choose_edge_action`. No AdaptiveParams. No morph mode.
#[derive(Debug, Clone)]
pub(crate) struct EdgeView {
    pub location: MemoryLocationHash,
    pub reader: TxIdx,
    pub access_k: u32,
    pub access_depth: u8,
    pub writer: Option<TxIdx>,
    pub bind_version: Option<TxVersion>,
    /// True when MV has non-ESTIMATE Data (A3). Not a Bind gate.
    pub writer_published: bool,
    /// True when writer is Validated — **not** a Bind gate (A3).
    pub writer_validated: bool,
    pub is_program: bool,
    pub in_hot_set: bool,
    /// A2: first wr/publish already broadcast Avoid for this ℓ.
    pub avoid_broadcast: bool,
    /// A2: first-wave canary still allowed (no Avoid yet, grant open).
    pub canary_ok: bool,
    /// A4: no predicted essential edge.
    pub independence_certified: bool,
    /// Known/predicted unpublished essential anti-dep.
    pub essential_antidep: bool,
    pub force_prefix: bool,
    /// A1: mass Spec on this clique is gated.
    pub clique_gated: bool,
}

/// Decide the protocol verb. Bounded optimism: known essential → Bind or WaitFor.
pub(crate) fn choose_edge_action(v: &EdgeView) -> EdgeAction {
    let _ = (v.location, v.reader, v.access_k, v.access_depth, v.is_program);
    // writer_validated is Detect state only — never a Bind door (A3).
    let _ = v.writer_validated;

    // A3: published Data (incl. unfinished / un-Validated tip) → Bind.
    if let Some(ver) = v.bind_version.clone() {
        return EdgeAction::Bind(ver);
    }
    if v.writer_published {
        if let Some(w) = v.writer {
            return EdgeAction::Bind(TxVersion {
                tx_idx: w,
                tx_incarnation: 0,
            });
        }
    }

    // D6 / A1 / A2: unpublished essential → WaitFor, never Spec+retry.
    let must_wait = v.essential_antidep
        || v.avoid_broadcast
        || v.force_prefix
        || (v.clique_gated && !v.canary_ok && !v.independence_certified)
        || (v.in_hot_set && !v.canary_ok && !v.independence_certified);
    if must_wait {
        if let Some(w) = v.writer {
            return EdgeAction::WaitFor(w);
        }
    }

    // A2 canary or A4 independence or cold discovery.
    EdgeAction::SpecRead
}

/// Multi-touch EdgeTable. Key = `(ℓ, reader, k, depth)`.
#[derive(Debug, Default)]
pub(crate) struct EdgeTable {
    edges: DashMap<EdgeKey, EdgeRec, FxBuildHasher>,
    /// Per-location Avoid verb (A2). Subsequent similar edges Avoid, not Spec.
    avoid: DashMap<MemoryLocationHash, (), FxBuildHasher>,
    /// Distinct access keys recorded (Detect completeness).
    access_count: AtomicUsize,
    avoid_count: AtomicUsize,
    wr_count: AtomicUsize,
    ww_count: AtomicUsize,
}

impl EdgeTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(
        &self,
        key: EdgeKey,
        writer: Option<TxIdx>,
        kind: EdgeKind,
        state: EdgeState,
    ) {
        let rec = EdgeRec {
            writer,
            kind,
            state,
        };
        if self.edges.insert(key, rec).is_none() {
            self.access_count.fetch_add(1, Ordering::Relaxed);
        }
        match kind {
            EdgeKind::Wr => {
                self.wr_count.fetch_add(1, Ordering::Relaxed);
            }
            EdgeKind::Ww => {
                self.ww_count.fetch_add(1, Ordering::Relaxed);
            }
            EdgeKind::Rw => {}
        }
    }

    /// A2: first confirmed publish → Avoid for later readers of ℓ.
    pub(crate) fn broadcast_avoid(&self, location: MemoryLocationHash) -> bool {
        if self.avoid.insert(location, ()).is_none() {
            self.avoid_count.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    #[inline]
    pub(crate) fn avoid_broadcast(&self, location: MemoryLocationHash) -> bool {
        self.avoid.contains_key(&location)
    }

    pub(crate) fn set_state(&self, key: &EdgeKey, state: EdgeState) {
        if let Some(mut e) = self.edges.get_mut(key) {
            e.state = state;
        }
    }

    /// Multi-touch: all accesses of `reader` on `ℓ` (not a single flatten slot).
    pub(crate) fn accesses_of(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
    ) -> Vec<(EdgeKey, EdgeRec)> {
        self.edges
            .iter()
            .filter(|e| e.key().location == location && e.key().reader == reader)
            .map(|e| (*e.key(), *e.value()))
            .collect()
    }

    pub(crate) fn access_count(&self) -> usize {
        self.access_count.load(Ordering::Relaxed)
    }

    pub(crate) fn avoid_count(&self) -> usize {
        self.avoid_count.load(Ordering::Relaxed)
    }

    pub(crate) fn wr_count(&self) -> usize {
        self.wr_count.load(Ordering::Relaxed)
    }

    pub(crate) fn ww_count(&self) -> usize {
        self.ww_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        bind: Option<TxVersion>,
        writer: Option<TxIdx>,
        published: bool,
        validated: bool,
        hot: bool,
        avoid: bool,
        canary: bool,
        indep: bool,
        essential: bool,
        clique: bool,
    ) -> EdgeView {
        EdgeView {
            location: 1,
            reader: 7,
            access_k: 3,
            access_depth: 1,
            writer,
            bind_version: bind,
            writer_published: published,
            writer_validated: validated,
            is_program: true,
            in_hot_set: hot,
            avoid_broadcast: avoid,
            canary_ok: canary,
            independence_certified: indep,
            essential_antidep: essential,
            force_prefix: false,
            clique_gated: clique,
        }
    }

    #[test]
    fn a3_bind_published_data_without_writer_done() {
        let v = TxVersion {
            tx_idx: 2,
            tx_incarnation: 1,
        };
        // Hypothesis fix: Data exists → Bind even when !validated / !done.
        let a = choose_edge_action(&view(
            Some(v.clone()),
            Some(2),
            true,
            false,
            true,
            true,
            false,
            false,
            true,
            true,
        ));
        assert_eq!(a, EdgeAction::Bind(v));
    }

    #[test]
    fn a3_bind_not_gated_on_validated() {
        let v = TxVersion {
            tx_idx: 1,
            tx_incarnation: 0,
        };
        let a = choose_edge_action(&view(
            Some(v.clone()),
            Some(1),
            true,
            false,
            true,
            false,
            false,
            false,
            false,
            false,
        ));
        assert_eq!(a, EdgeAction::Bind(v));
    }

    #[test]
    fn d6_wait_unpublished_essential() {
        let a = choose_edge_action(&view(
            None, Some(3), false, false, true, false, false, false, true, false,
        ));
        assert_eq!(a, EdgeAction::WaitFor(3));
    }

    #[test]
    fn a2_avoid_broadcast_waits_not_spec() {
        let a = choose_edge_action(&view(
            None, Some(4), false, false, true, true, false, false, false, false,
        ));
        assert_eq!(a, EdgeAction::WaitFor(4));
    }

    #[test]
    fn a4_independence_specs() {
        let a = choose_edge_action(&view(
            None, None, false, false, false, false, false, true, false, false,
        ));
        assert_eq!(a, EdgeAction::SpecRead);
    }

    #[test]
    fn a2_canary_specs_before_avoid() {
        let a = choose_edge_action(&view(
            None, Some(1), false, false, true, false, true, false, false, true,
        ));
        assert_eq!(a, EdgeAction::SpecRead);
    }

    #[test]
    fn a1_clique_gate_waits_after_canary() {
        let a = choose_edge_action(&view(
            None, Some(1), false, false, true, false, false, false, false, true,
        ));
        assert_eq!(a, EdgeAction::WaitFor(1));
    }

    #[test]
    fn never_spec_known_essential() {
        let a = choose_edge_action(&view(
            None, Some(9), false, false, true, false, false, false, true, false,
        ));
        assert!(matches!(a, EdgeAction::WaitFor(9)));
        assert!(!matches!(a, EdgeAction::SpecRead));
    }

    #[test]
    fn edge_table_multi_touch_not_flat() {
        let t = EdgeTable::new();
        let k0 = EdgeKey {
            location: 5,
            reader: 10,
            access_k: 1,
            depth: 0,
        };
        let k1 = EdgeKey {
            location: 5,
            reader: 10,
            access_k: 4,
            depth: 2,
        };
        t.record(k0, Some(2), EdgeKind::Wr, EdgeState::Unpublished);
        t.record(k1, Some(2), EdgeKind::Wr, EdgeState::PublishedUncommitted);
        let acc = t.accesses_of(5, 10);
        assert_eq!(acc.len(), 2, "flatten (ℓ,reader) would drop a frame");
        assert!(t.broadcast_avoid(5));
        assert!(t.avoid_broadcast(5));
        assert!(!t.broadcast_avoid(5));
        assert_eq!(t.avoid_count(), 1);
    }
}
