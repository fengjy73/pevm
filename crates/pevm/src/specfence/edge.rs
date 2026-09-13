//! Frozen-grain Detect + `choose_edge_action` (v4.1-frozen π).
//!
//! **Spec = Region** (this `EdgeKey`), not speculate. Optimistic access is
//! [`EdgeAction::Unfenced`] ≡ OCC-cost for **this** access when
//! ¬PredictedEssential. Fence = Bind / WaitFor / serial-lane / ordered-admit
//! on **this** \(a\) only — never sticky Wait-on-tx.
//!
//! Frozen π:
//! ```text
//! a     = (t, k, depth, ℓ, mode)          # inc NOT Avoid key
//! e_vis = (writer?, published_Data?, kind)
//! gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
//! ```
//!
//! Conflict identity is **not** a flat `(ℓ, reader)` and not `inc` / ForcePrefix:
//! - `L_record` = location hash
//! - `L_access` = `(reader, k, depth)` (multi-touch / frames)
//! - `L_edge` = typed wr/rw/ww with unpublished | published-uncommitted | validated
//!
//! Live verbs read **only** π fields. `force_prefix` / canary / H / morph /
//! `writer_validated` / `inc` are observe-only or excluded — never OR-doors.
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
/// Spec is the Region (`EdgeKey`); it is not a verb on this enum.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EdgeAction {
    /// Fence: install a published version (A3). Writer need not be Validated.
    Bind(TxVersion),
    /// Fence: WaitFor unpublished PredictedEssential writer (this \(a\) only).
    WaitFor(TxIdx),
    /// No Region barrier — ¬PredictedEssential / independence ≡ OCC for this \(a\).
    /// Not Spec. Spec = Region. Never a canary / ForcePrefix tax class.
    Unfenced,
}

/// Access-class bucket for PredictedEssential(\(ℓ,k,\mathrm{morph}\)).
/// Same buckets as the 99-block field table (`k1_3` / `k16p` / …).
#[inline]
pub(crate) fn access_k_class(k: u32) -> u8 {
    match k {
        0 => 0,
        1..=3 => 1,
        4..=7 => 2,
        8..=15 => 3,
        _ => 4,
    }
}

/// Features for `choose_edge_action`. Live π fields vs observe/exclude.
///
/// **Enter π:** \(a=(t,k,\mathrm{depth},ℓ)\), \(e_{\mathrm{vis}}\),
/// `predicted_essential`, `independence_certified`.
/// **Observe only:** H, prior_warm, canary, clique, force_prefix, morph,
/// `writer_validated`, Ready/Executing, avoid-as-ℓ-sticky.
/// **Exclude as Avoid keys:** `inc`, canary verb, `force_prefix`, H-OR,
/// morph actuator, `writer_validated` Bind gate, flat `(ℓ,reader)`.
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
    /// Observe / metric only — **not** a Bind gate (A3).
    pub writer_validated: bool,
    pub is_program: bool,
    /// Observe / prior only — **banned** as Wait OR-door.
    pub in_hot_set: bool,
    /// Observe: location-wide first-wave flag. **Not** a live OR-door
    /// (per-access-class PredictedEssential is the gate).
    pub avoid_broadcast: bool,
    /// Exclude: canary as live verb. Always ignored by classify.
    pub canary_ok: bool,
    /// A4: ¬PredictedEssential certificate → Unfenced≡OCC.
    pub independence_certified: bool,
    /// Live gate: PredictedEssential(\(ℓ,k,\mathrm{morph}\)) for **this** \(a\).
    pub predicted_essential: bool,
    /// Alias of `predicted_essential` for DecisionFieldAgg (lab).
    pub essential_antidep: bool,
    /// Exclude: ForcePrefix bool is **not** π (metrics → 0).
    pub force_prefix: bool,
    /// Observe: clique / canary-adjacent. **Not** an Avoid key.
    pub clique_gated: bool,
    /// Observe: writer Executing (ordered-admit heat, not a verb).
    pub writer_executing: bool,
    /// Observe: writer Ready (ordered-admit, not Unfenced hang-freedom).
    pub writer_ready: bool,
}

/// Version-visibility state (native CC). Reasons are metrics-only.
/// Not `must_wait = force_prefix∨avoid∨H∨canary∨inc` — classify first, then one verb.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EdgeVisibility {
    /// Published Data (incl. Executed-not-Validated tip) → Bind.
    PublishedData {
        version: TxVersion,
    },
    /// PredictedEssential ∧ writer? ∧ ¬published ∧ \(w < reader\).
    UnpublishedEssential {
        writer: TxIdx,
    },
    /// PredictedEssential ∧ unpublished ∧ a **real** serial-lane pred
    /// (writer identity). Never `reader-1` ghost (wait_no_writer smell).
    SerialLane {
        pred: TxIdx,
    },
    Independent,
    Cold,
}

/// Classify \(a + e_{\mathrm{vis}} + \mathrm{gate}\) into version-visibility.
///
/// Exclude-set fields (`force_prefix`, canary, H, clique, `writer_validated`,
/// location-wide avoid) are recorded then discarded — they never OR into Fence.
/// Hang-freedom is serial-lane / ordered-admit / steal, never Unfenced-on-essential
/// and never WaitFor without a writer identity.
pub(crate) fn classify_edge(v: &EdgeView) -> EdgeVisibility {
    let _ = (
        v.location,
        v.reader,
        v.access_k,
        v.access_depth,
        v.is_program,
    );
    // Exclude / observe — never live OR-bools.
    let _ = v.writer_validated;
    let _ = (v.writer_executing, v.writer_ready);
    let _ = v.in_hot_set;
    let _ = v.force_prefix;
    let _ = v.canary_ok;
    let _ = v.clique_gated;
    let _ = v.avoid_broadcast;

    // gate: PredictedEssential(ℓ, k, morph) for THIS a only.
    let predicted = v.predicted_essential || v.essential_antidep;

    // e_vis Bind only under PredictedEssential (A3: Data→Bind, no
    // writer_validated gate). ¬PredictedEssential must stay Unfenced≡OCC
    // even when MV Data exists — that is the hybrid law (no Bind tax on cold).
    if predicted {
        if let Some(version) = v.bind_version.clone() {
            return EdgeVisibility::PublishedData { version };
        }
        if v.writer_published {
            if let Some(w) = v.writer {
                return EdgeVisibility::PublishedData {
                    version: TxVersion {
                        tx_idx: w,
                        tx_incarnation: 0,
                    },
                };
            }
        }
        if let Some(w) = v.writer {
            if w < v.reader {
                return EdgeVisibility::UnpublishedEssential { writer: w };
            }
            // inversion: later/self writer is not a preset-order anti-dep
        }
        // No writer identity → do **not** WaitFor(reader-1). Serial-lane /
        // ordered-admit happen in the scheduler; classify stays Unfenced≡OCC
        // rather than invent wait_no_writer.
    }

    if v.independence_certified || !predicted {
        return EdgeVisibility::Independent;
    }
    EdgeVisibility::Cold
}

/// Decide the protocol verb from frozen π visibility.
/// PredictedEssential ∧ Data → Bind; PredictedEssential ∧ writer → WaitFor;
/// else Unfenced≡OCC. Canary / ForcePrefix / SerialLane-without-writer are gone.
pub(crate) fn choose_edge_action(v: &EdgeView) -> EdgeAction {
    match classify_edge(v) {
        EdgeVisibility::PublishedData { version } => EdgeAction::Bind(version),
        EdgeVisibility::UnpublishedEssential { writer } => EdgeAction::WaitFor(writer),
        EdgeVisibility::SerialLane { pred } => EdgeAction::WaitFor(pred),
        EdgeVisibility::Independent | EdgeVisibility::Cold => EdgeAction::Unfenced,
    }
}

/// Multi-touch EdgeTable. Key = `(ℓ, reader, k, depth)`.
#[derive(Debug, Default)]
pub(crate) struct EdgeTable {
    edges: DashMap<EdgeKey, EdgeRec, FxBuildHasher>,
    /// Per-location Avoid verb (A2). Subsequent similar edges Fence, not Unfenced.
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

    /// A5: min access `k` among invalid ℓ for this reader (piece-restricted R2).
    pub(crate) fn min_k_of_invalid(
        &self,
        reader: TxIdx,
        invalid: &[MemoryLocationHash],
    ) -> Option<usize> {
        let mut min_k: Option<u32> = None;
        for &loc in invalid {
            for (key, _) in self.accesses_of(loc, reader) {
                min_k = Some(min_k.map_or(key.access_k, |m| m.min(key.access_k)));
            }
        }
        min_k.map(|k| k as usize)
    }

    /// Multi-touch: later frames of the same `(ℓ, reader)` after `access_k`.
    pub(crate) fn later_touches(
        &self,
        location: MemoryLocationHash,
        reader: TxIdx,
        access_k: u32,
    ) -> usize {
        self.edges
            .iter()
            .filter(|e| {
                e.key().location == location
                    && e.key().reader == reader
                    && e.key().access_k > access_k
            })
            .count()
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
            predicted_essential: essential,
            essential_antidep: essential,
            force_prefix: false,
            clique_gated: clique,
            writer_executing: false,
            writer_ready: false,
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
        // A3 still holds — but only on the PredictedEssential path.
        let a = choose_edge_action(&view(
            Some(v.clone()),
            Some(1),
            true,
            false,
            true,
            false,
            false,
            false,
            true,
            false,
        ));
        assert_eq!(a, EdgeAction::Bind(v));
    }

    #[test]
    fn unfenced_when_data_but_not_predicted() {
        let v = TxVersion {
            tx_idx: 1,
            tx_incarnation: 0,
        };
        let a = choose_edge_action(&view(
            Some(v),
            Some(1),
            true,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        ));
        assert_eq!(
            a,
            EdgeAction::Unfenced,
            "¬PredictedEssential must not Bind-tax published Data"
        );
    }

    #[test]
    fn d6_wait_unpublished_essential() {
        let a = choose_edge_action(&view(
            None,
            Some(3),
            false,
            false,
            true,
            false,
            false,
            false,
            true,
            false,
        ));
        assert_eq!(a, EdgeAction::WaitFor(3));
    }

    #[test]
    fn location_avoid_is_not_an_or_door() {
        // Per-ℓ Avoid broadcast is observe / first-wave fuel — not a live OR.
        // Without PredictedEssential(ℓ, k) this access stays Unfenced≡OCC.
        let a = choose_edge_action(&view(
            None,
            Some(4),
            false,
            false,
            true,
            true,
            false,
            true,
            false,
            false,
        ));
        assert_eq!(a, EdgeAction::Unfenced);
    }

    #[test]
    fn a4_independence_unfenced() {
        let a = choose_edge_action(&view(
            None, None, false, false, false, false, false, true, false, false,
        ));
        assert_eq!(a, EdgeAction::Unfenced);
    }

    #[test]
    fn canary_is_not_a_live_verb() {
        // Exclude: canary never enters classify (was Unfenced discovery tax).
        let a = choose_edge_action(&view(
            None,
            Some(1),
            false,
            false,
            true,
            false,
            true,
            true,
            false,
            true,
        ));
        assert_eq!(a, EdgeAction::Unfenced);
        assert!(matches!(
            classify_edge(&view(
                None,
                Some(1),
                false,
                false,
                true,
                false,
                true,
                true,
                false,
                true,
            )),
            EdgeVisibility::Independent
        ));
    }

    #[test]
    fn clique_is_not_an_avoid_key() {
        // Clique / canary-adjacent is observe-only. ¬PredictedEssential → OCC.
        let a = choose_edge_action(&view(
            None,
            Some(1),
            false,
            false,
            true,
            false,
            false,
            true,
            false,
            true,
        ));
        assert_eq!(a, EdgeAction::Unfenced);
    }

    #[test]
    fn clique_none_writer_first_wave_unfenced() {
        let a = choose_edge_action(&view(
            None, None, false, false, false, false, false, true, false, true,
        ));
        assert_eq!(
            a,
            EdgeAction::Unfenced,
            "clique ∧ writer=None ∧ !PredictedEssential is OCC, not serial-all"
        );
    }

    #[test]
    fn independence_wins_without_predicted_essential() {
        // Clique + writer without PredictedEssential stays Unfenced≡OCC.
        let a = choose_edge_action(&view(
            None,
            Some(2),
            false,
            false,
            false,
            false,
            false,
            true,
            false,
            true,
        ));
        assert_eq!(
            a,
            EdgeAction::Unfenced,
            "¬PredictedEssential must not Fence via clique/canary leftover"
        );
    }

    #[test]
    fn never_wait_for_higher_or_self() {
        let mut v = view(
            None,
            Some(9),
            false,
            false,
            true,
            true,
            false,
            false,
            true,
            true,
        );
        v.reader = 4;
        v.writer = Some(9);
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Unfenced,
            "WaitFor(later) inverts preset order"
        );
        v.writer = Some(4);
        assert_eq!(choose_edge_action(&v), EdgeAction::Unfenced);
    }

    #[test]
    fn wait_for_ready_known_essential() {
        // Ready is not an Unfenced door — admission makes the writer progress.
        let a = choose_edge_action(&view(
            None,
            Some(3),
            false,
            false,
            true,
            true,
            false,
            false,
            true,
            true,
        ));
        assert_eq!(a, EdgeAction::WaitFor(3));
    }

    #[test]
    fn hot_alone_is_cold_unfenced_not_wait() {
        // H membership is not an OR-door into WaitFor (dissolved salad).
        let a = choose_edge_action(&view(
            None, None, false, false, true, false, false, false, false, false,
        ));
        assert_eq!(a, EdgeAction::Unfenced);
        assert!(matches!(
            classify_edge(&view(
                None, None, false, false, true, false, false, false, false, false,
            )),
            EdgeVisibility::Independent | EdgeVisibility::Cold
        ));
    }

    #[test]
    fn visibility_machine_published_then_essential_then_unfenced() {
        let v = TxVersion {
            tx_idx: 2,
            tx_incarnation: 1,
        };
        assert!(matches!(
            classify_edge(&view(
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
            )),
            EdgeVisibility::PublishedData { .. }
        ));
        assert!(matches!(
            classify_edge(&view(
                None,
                Some(3),
                false,
                false,
                true,
                false,
                false,
                false,
                true,
                false,
            )),
            EdgeVisibility::UnpublishedEssential { writer: 3 }
        ));
        assert!(matches!(
            classify_edge(&view(
                None, None, false, false, false, false, false, true, false, false,
            )),
            EdgeVisibility::Independent
        ));
    }

    #[test]
    fn never_unfenced_known_essential() {
        let a = choose_edge_action(&view(
            None,
            Some(3),
            false,
            false,
            true,
            false,
            false,
            false,
            true,
            false,
        ));
        assert!(matches!(a, EdgeAction::WaitFor(3)));
        assert!(!matches!(a, EdgeAction::Unfenced));
    }

    #[test]
    fn force_prefix_is_not_pi() {
        // Exclude: ForcePrefix bool must not Fence (sticky tx-grain / inc smell).
        let mut v = view(
            None, None, false, false, true, false, false, true, false, false,
        );
        v.force_prefix = true;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Unfenced,
            "force_prefix ∧ writer=None is NOT serial-lane π"
        );
        v.reader = 0;
        assert_eq!(choose_edge_action(&v), EdgeAction::Unfenced);
    }

    #[test]
    fn force_prefix_does_not_override_independence() {
        let mut v = view(
            None,
            Some(2),
            false,
            false,
            true,
            false,
            false,
            true,
            false,
            false,
        );
        v.force_prefix = true;
        v.writer_ready = true;
        v.independence_certified = true;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Unfenced,
            "force_prefix is excluded; ¬PredictedEssential → OCC"
        );
        v.writer_executing = true;
        v.writer_ready = false;
        assert_eq!(choose_edge_action(&v), EdgeAction::Unfenced);
    }

    #[test]
    fn prefer_admit_ready_does_not_unfence_must_wait() {
        let mut v = view(
            None,
            Some(3),
            false,
            false,
            true,
            true,
            false,
            true,
            true,
            true,
        );
        v.writer_ready = true;
        v.writer_executing = false;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::WaitFor(3),
            "S1+U3: Ready spine writer stays a Fence; admit is the hang door"
        );
    }

    #[test]
    fn avoid_none_writer_does_not_ghost_wait() {
        // wait_no_writer smell: no writer identity → no WaitFor(reader-1).
        let a = choose_edge_action(&view(
            None, None, false, false, true, true, false, false, true, false,
        ));
        assert_eq!(
            a,
            EdgeAction::Unfenced,
            "PredictedEssential ∧ writer=None must not invent a serial pred"
        );
    }

    #[test]
    fn predicted_essential_waits_this_access_only() {
        let mut v = view(
            None,
            Some(3),
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            false,
        );
        v.access_k = 6;
        v.access_depth = 2;
        assert_eq!(choose_edge_action(&v), EdgeAction::WaitFor(3));
        // Sibling access in the same tx (different k) is ¬PredictedEssential.
        v.predicted_essential = false;
        v.essential_antidep = false;
        v.independence_certified = true;
        v.access_k = 12;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Unfenced,
            "mixed verbs inside one tx: later k stays OCC"
        );
    }

    #[test]
    fn inc_is_not_in_avoid_key() {
        // EdgeKey / EdgeView have no incarnation field. Repair-state must not
        // re-key Avoid (99-block: inc1+ fence≈74% smell).
        let k0 = EdgeKey {
            location: 1,
            reader: 7,
            access_k: 6,
            depth: 1,
        };
        let k1 = EdgeKey {
            location: 1,
            reader: 7,
            access_k: 6,
            depth: 1,
        };
        assert_eq!(k0, k1, "same a=(t,k,depth,ℓ) — inc is not part of identity");
    }

    #[test]
    fn h_is_not_a_wait_or_door() {
        let mut v = view(
            None,
            Some(2),
            false,
            false,
            true,
            false,
            false,
            true,
            false,
            false,
        );
        v.in_hot_set = true;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Unfenced,
            "H membership is observe/prior, not a Wait OR-door"
        );
    }

    #[test]
    fn writer_validated_is_not_a_bind_gate() {
        let ver = TxVersion {
            tx_idx: 2,
            tx_incarnation: 0,
        };
        let mut v = view(
            Some(ver.clone()),
            Some(2),
            true,
            false,
            false,
            false,
            false,
            false,
            true,
            false,
        );
        v.writer_validated = false;
        assert_eq!(
            choose_edge_action(&v),
            EdgeAction::Bind(ver),
            "A3: Data → Bind even if !validated"
        );
    }

    #[test]
    fn access_k_class_buckets_match_field_table() {
        assert_eq!(access_k_class(0), 0);
        assert_eq!(access_k_class(2), 1);
        assert_eq!(access_k_class(6), 2);
        assert_eq!(access_k_class(10), 3);
        assert_eq!(access_k_class(20), 4);
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
        assert_eq!(t.min_k_of_invalid(10, &[5]), Some(1));
        assert_eq!(t.later_touches(5, 10, 1), 1);
    }
}
