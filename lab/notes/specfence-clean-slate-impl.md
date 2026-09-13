# SpecFence clean-slate — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-clean-slate-8598` (PR #5 → `cursor/specfence-frozen-grain-3175` / PR #4)  
**Plant SoT:** `lab/notes/specfence-clean-slate-architecture.md`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (unchanged)  
**Honesty baseline:** cost-class all-blocks nonempty N=1 median **SF/OCC = 0.436** (`d3a7ae1`)

**Verdict:** one-iteration plant restructure landed. **Do not claim quiet≡1.0 or median ≥0.7.** Nonempty median **0.464** (↑ vs 0.436). Quiet heuristic median **0.716** (20/48 ≥1) — not ≈1.0. OCC mode detect/pcc/unfenced = **0**.

---

## Land map (file:fn)

| # | SoT item | file:fn | Status |
|---|---------|---------|--------|
| 1 | Architecture SoT first | `lab/notes/specfence-clean-slate-architecture.md` | **landed** |
| 2 | OCC `maybe_wait` = `Ok(())`, zero SF calls | `vm.rs::maybe_wait` | **landed** |
| 3 | OCC execute skips `record_evm_entry` / `ff_continuation` | `vm.rs::execute` | **landed** |
| 4 | Hinted account Wait is PCC-legacy only | `executor.rs::hinted_wait_enabled` | **landed** |
| 5 | SpecFence access gate in `access_policy` | `access_policy.rs::decide` | **landed** |
| 6 | Unfenced ⇒ `occ_unfenced` (no rem `first_k` / journal / process DashMap / `note_detect` / residual-Bind) | `vm.rs::occ_unfenced` / `specfence_access_gate` | **landed** |
| 7 | PCC overlay only on PE ∩ ROI | `vm.rs::pcc_overlay` | **landed** |
| 8 | First-incarnation Unfenced ESTIMATE → Blocking (OCC) | `vm.rs::storage` / `basic` (`resolve_read_overlay`) | **landed** |
| 9 | PrefixSkip / FF-head = Resolve overlay, not Unfenced | `vm.rs::resolve_read_overlay` | **landed** |
| 10 | Detect coverage atomic only on Unfenced; DashMap off path | `specfence_access_gate` | **landed** |
| 11 | mixed_verb via end-tx flush, not per-SLOAD DashMap | `process.rs::note_unfenced_occ` / `flush_access_census` | **landed** |
| 12 | Cheap `bump_k` only when PE table nonempty | `rem.rs::bump_k_only` | **landed** |
| 13 | Executor tick helpers | `executor.rs` | **landed** |
| 14 | Empty-PE plant skips extra `bump_k` | `specfence_plant_is_occ` | **landed** |

**Not landed (seq≠par if stripped):** rem write journal / CallEntry / `collect_invalid_reads` on every SpecFence validate / HotSet observe. Those remain the quiet residual vs OCC.

---

## Control loop (live)

```
OCC access:     Ok(()) then occ storage/basic (ESTIMATE→Blocking)
SpecFence a:
  detect_coverage += 1
  if !has_any_predicted: occ_unfenced + OCC MV read
  else:
    k = bump_k()
    if decide() == Unfenced: occ_unfenced + OCC MV read
    else: pcc_overlay (Bind | WaitFor)
  if rewind_resume | ff_head: Resolve FF / OrderedDirtyRead
```

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release -- --test-threads=1` | 159 ok |
| `cargo +nightly test -p pevm --test specfence --release -- --test-threads=1` | 42 ok / 20 ignored (full-suite env-var flakes isolated; tests pass `--exact`) |

---

## Honesty vs 0.436

Sweeps (gitignored JSON):  
`lab/results/clean-slate-all-blocks-sweep.json`,  
`lab/results/clean-slate-focus-sweep.json`.  
Committed digest: `lab/notes/clean-slate-sweep-summary.json`.

### All-blocks N=1 @8 (98 nonempty / 99 loaded)

Empty snapshot **19910734** (`n_tx=0`, printed `sf_occ=0`) is dropped from the nonempty median — same rule as cost-class.

| | This cut | Cost-class to beat |
|--|----------|-------------------|
| median SF/OCC | **0.464** | **0.436** |
| p10 / min (nonempty) | 0.288 / **0.206** | 0.256 / **0.149** (6196166) |
| mean | 1.36 | 1.33 |
| quiet heuristic (48 nonempty) median | **0.716** (20/48 ≥1) | 0.989 (16/33 ≥1) |
| fan_out (18) median | **0.399** | 0.423 (53 — cohort cut differs) |
| ≥0.7 / ≥1.0 | 29 / 21 | — |

Median **0.436 → 0.464**. Soft=0, await=0. Exclude-set = 0 on all SF rows. Detect 314 350; `unfenced_occ_fast` = `edge_unfenced` = 327 551; `pcc_fire_at_a` 12 920; `pcc_roi_skip` 223.

**OCC mode:** `detect_accesses` = `pcc_fire_at_a` = `unfenced_occ_fast` = 0 (pristine).

| Block | Role | SF/OCC (N=1) | cost-class N=1 | notes |
|------:|------|-------------:|---------------:|-------|
| 19807137 | worst | **0.323** | 0.252 | wall 98 vs 32 ms; WaitFor≈0 |
| 6196166 | park | **0.317** | 0.149 | still abort-heavy |
| 14689597 | focus | **0.229** | 0.227 | bind=227; B0-heavy |
| 2179522 | quiet | **0.405** | 3.56 (OCC N=1 pathology) | bind=0; SF 4.5 vs OCC 1.8 ms — **Unfenced≢OCC wall** |

### Focus+worst+quiet N=3 (8-block set)

Printed median **0.426** / mean **0.375** / min **0.204**. Soft=0. Prior cost-class N=3 median **0.424** / min **0.097**.

| Block | Role | SF/OCC | cost-class N=3 |
|------:|------|-------:|---------------:|
| 14689597 | focus | 0.233 | 0.209 |
| 19606599 | focus | 0.533 | 0.517 |
| 19469097 | focus | 0.431 | 0.424 |
| 19807137 | worst | 0.204 | 0.200 |
| 6196166 | park | 0.426 | 0.097 |
| 6137495 | worst-ish | 0.330 | 0.240 |
| 2179522 | quiet | **0.385** | **1.05** |
| 19606598 | quiet neighbor | 0.460 | 0.472 |

Quiet **regressed vs cost-class N=3 1.05** on this 8-block set: SF still pays rem-write + `collect_invalid_reads` + execute theater after the Unfenced read path was unified. Stripping those on empty-PE / quiet_fence_off **failed seq≡par** in ERC-20 / Bind tests — left in place, documented as remaining graft.

---

## Remaining (not leftover π)

1. Quiet wall ≫ OCC (2179522 N=1 **0.405**, N=3 **0.385**) — shared execute/validate/rem-write, not Edge-on-SLOAD.  
2. Fan_out still 0.23–0.40 — PCC Bind/B0 vs OCC reincarnation.  
3. `collect_invalid_reads` on every SpecFence validate (OCC-first validate broke seq≡par on ERC-20).  
4. Mean inflated by 19434587 OCC N=1 pathology (72×).

Plant SoT invariants held: OCC zero SF counters; Unfenced first incarnation is OCC ESTIMATE→Blocking; PCC only on PE ∩ ROI; exclude-set 0; Soft=0.
