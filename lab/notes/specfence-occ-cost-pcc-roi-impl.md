# SpecFence OCC-cost + PCC-ROI — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-frozen-grain-3175` (PR #4)  
**SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`  
**Grain land (π, already frozen):** `lab/notes/specfence-frozen-grain-impl.md`  
**Vocab:** Spec = Region; Fence = Bind / WaitFor / serial-lane / ordered-admit; Unfenced ≡ OCC-cost for **this** access when ¬PredictedEssential **or** Fence_tax ≥ OCC_reexec.

**Verdict:** one iteration, all cost-class problems. No new π fields. Frozen π identity unchanged.

```
a = (t, k, depth, ℓ, mode)                    # inc NOT Avoid key
e_vis = (writer?, published_Data?, edge_kind)
gate  = PredictedEssential(ℓ, k, morph) ∨ independence_certified
verb  = Bind | WaitFor|serial-lane|ordered_admit | Unfenced≡OCC
        # THIS a only — PCC Fire only if Fence_tax < OCC_reexec
```

Honesty baseline this cut beats: frozen-grain all-blocks N=1 median **SF/OCC = 0.351**.

---

## Root cause (why 0.35×)

1. **Unfenced ≢ OCC.** `maybe_wait_specfence` paid Edge SM / `edges.record` / `choose_edge_action` / `process.record_decision` / sketch consult / `note_detect` DashMap / `note_pending_effect_boundary` (alloc + checkpoint) / PreferAdmit on **every** ¬PE access. OCC `maybe_wait` is two branches and `Ok`.
2. **PCC Fire > OCC reincarnation.** PredictedEssential (incl. prior-only) → WaitFor park + PrefixSkip/rewind + validate yield-spins + validation-defer. 19807137 wall was rewind/park, not missing PE.

---

## Land map (file:fn)

| # | Item | file:fn | Status |
|---|------|---------|--------|
| A1 | Unfenced ≡ OCC: empty PE or ROI miss → `unfenced_occ_fast` | `vm.rs::maybe_wait_specfence` / `unfenced_occ_fast` | **landed** |
| A2 | Detect: always `record_detect_access`; `note_detect` sampled 1/16 | `vm.rs` + `learner.rs::note_detect` | **landed** |
| A3 | rem \(k\) + first_k without journal/checkpoint | `rem.rs::note_access_k_only` | **landed** |
| A4 | No Edge / sketch / PreferAdmit / canary / process DashMap on Unfenced | `vm.rs::unfenced_occ_fast` | **landed** |
| A5 | Cheap PE emptiness (`predicted_n`) | `learner.rs::has_any_predicted` | **landed** |
| B1 | PCC Fire iff intra abort PE **and** !park/rewind storm | `learner.rs::pcc_makespan_win` | **landed** |
| B2 | Bind-on-Data only after intra evidence (prior-only stays OCC) | `vm.rs::pcc_bind_published` | **landed** |
| B3 | WaitFor only single executing writer; no fleet PreferAdmit | `learner.rs::waitfor_makespan_win` + `vm.rs::pcc_wait_for_writer` | **landed** |
| B4 | PrefixSkip only if cheaper than B0; else OCC reincarnation | `rem.rs::prefix_skip_beats_b0` / `apply_suffix_repair_planned` | **landed** |
| B5 | No validate yield-spin / validation-defer park / fanout absorb | `pevm.rs::try_validate` | **landed** |
| C | Frozen π / Soft=0 / PE not from Detect publish / no bn hardcode | unchanged | **held** |

---

## Control loop (live)

```
access_tick a:
  detect_accesses += 1                         # cheap atomic
  note_detect sampled 1/16                     # observe; never plants PE
  if !has_any_predicted ∨ ¬PE(ℓ,k) ∨ quiet∧¬intra:
      Unfenced≡OCC                             # k-only rem; return
  if ¬pcc_makespan_win(ℓ,k):                   # prior-only / park storm
      Unfenced≡OCC
  if published Data:
      Bind                                     # cheap PCC
  else if waitfor_makespan_win(1, executing):
      WaitFor(w) + admit w only                # no fleet
  else:
      Unfenced≡OCC                             # miss → cheap reincarnation

resolve_tick:
  R1a RebindThis if value already stable       # no yield-spin
  R1b PrefixSkip iff prefix_skip_beats_b0      # cp.k≥8, first repair, ≥half grain
  else B0 FullRestart                          # OCC-identical
```

`prefix_skip_beats_b0(cp_k, k_fail, depth)` =
`depth==0 ∧ cp_k≥8 ∧ k_fail>cp_k+2 ∧ 2·cp_k≥k_fail`.

---

## Hard-ban checklist (held)

| Ban | This cut |
|-----|----------|
| SoftWait Soft | not armed |
| Tx sticky Wait / ForcePrefix π / canary / H-OR / inc Avoid | unchanged exclude-set |
| Detect publish plants PE | unchanged |
| Unfenced cost > OCC | Unfenced bypasses Edge/sketch/checkpoint |
| SuffixRepair default | B0 unless PrefixSkip cheaper |
| Fleet WaitFor / PreferAdmit independents | WaitFor target only |
| 597 / bn hardcodes | none |

---

## Tests

| Suite | Expect |
|-------|--------|
| `learner.rs` | `pcc_makespan_win` requires intra abort; WaitFor rejects fleet / park storm |
| `rem.rs` | tiny prefix → B0; substantial prefix → PrefixSkip; `prefix_skip_beats_b0` |
| `edge.rs` | frozen π SM unchanged |
| `cargo test -p pevm --lib --release` | green |
| `cargo test -p pevm --test specfence --release -- --test-threads=1` | green |

Sweep JSON (gitignored):  
`lab/results/occ-cost-all-blocks-sweep.json`,  
`lab/results/occ-cost-focus-n3-sweep.json`.  
Tip `5ef6791` + test-assert follow-up.

---

## Honesty vs 0.351 median

Do **not** claim ≥0.7 or SoT “then → ≥1.0”. Mean is inflated by N=1 OCC pathology (e.g. 19434587). Empty snapshot 19910734 (`n_tx=0`) is dropped from the nonempty median.

### All-blocks N=1 @8 (98 nonempty / 99 loaded)

| | This cut | Grain tip to beat |
|--|----------|-------------------|
| median SF/OCC | **0.436** | **0.351** |
| p10 / min (nonempty) | 0.256 / **0.149** (6196166) | 0.215 / **0.057** (19807137) |
| mean | 1.33 | 0.98 |
| quiet heuristic (33 nonempty) median | **0.989** (16/33 ≥1) | 1.07 (18/34 ≥1) |
| fan_out (53) median | **0.423** | 0.315 |

Median **rose 0.351 → 0.436** toward ≥0.5. Soft=0, await=0. Exclude-set counters = 0 on all 99 SF rows. Detect 324 100; `unfenced_occ_fast` 244 266 (= `edge_unfenced`); `pcc_fire_at_a` 15 093; `pcc_roi_skip` 1 524; `prefix_skip_roi_b0` 13 259; rewind≈0 on worst (19807137 rewind=0, B0=2853).

| Block | Role | SF/OCC (N=1) | grain N=1 | notes |
|------:|------|-------------:|----------:|-------|
| 19807137 | worst | **0.252** | 0.057 | wall 76 vs 19 ms; WaitFor=0; PrefixSkip default gone |
| 6196166 | park | **0.149** | ~0.10 focus | still Bind-residual / abort-heavy; min of set |
| 14689597 | focus | **0.227** | 0.345 | more B0; mixed_verb=260 |
| 2179522 | quiet | 3.56 (OCC N=1 pathology) | 0.40 | N=3 quiet **1.05**; pcc_fire=0 |

### Focus+worst+quiet N=3 (8-block set)

Printed median **0.424** / mean **0.401** / min **0.097**. Soft=0. Prior grain N=3 median **0.307** / min **0.085**.

| Block | Role | SF/OCC | grain N=3 | notes |
|------:|------|-------:|----------:|-------|
| 14689597 | focus | 0.209 | 0.345 | B0-heavy |
| 19606599 | focus | 0.517 | 0.307 | up |
| 19469097 | focus | 0.424 | 0.293 | up |
| 19807137 | worst | **0.200** | **0.085** | 2.4×; rewind=0 |
| 6196166 | park | 0.097 | 0.097 | still park/abort class |
| 6137495 | worst-ish | 0.240 | 0.210 | |
| 2179522 | quiet | **1.05** | 1.57 | SF wall 2.8 ≈ OCC 2.9 |
| 19606598 | quiet neighbor | 0.472 | 0.347 | up |

Falsifiers held: Soft=0; exclude-set=0; quiet Unfenced≡OCC (`pcc_fire=0` on 2179522); fan_out Unfenced path is `unfenced_occ_fast` (no Edge tax); mixed_verb 14689597=260, 19807137=305.

Remaining performance (not leftover π): 6196166 / some fan_out still pay Bind-residual on reincarnation and extra B0 vs OCC. Cost class is the land; median moved 0.351 → 0.436.
