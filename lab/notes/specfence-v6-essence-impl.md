# SpecFence v6 essence — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v6-essence-af82`  
**Base:** `cursor/specfence-v5-pc-cc-fusion-e28e` @ `9a49b5f` (PR #7) + SoT docs `b4e4010`  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v6-essence.md`  
**Diagnosis absorbed:**  
`lab/notes/specfence-v5-regression-all-blocks-diagnosis.md`  
`lab/notes/specfence-v5-mode-a-switch-path-audit.md`  
`lab/notes/specfence-v5-regression-per-block-catalog.json`

**Honesty bar:** nonempty median SF/OCC **> 0.744**; Soft=0; seq≡par.  
Named: 14689597 / 2179522 / 19807137.

---

## Land map (file:fn)

| # | SoT item | file:fn | Status |
|---|---------|---------|--------|
| 1 | `access_vis.unfinished` = **!done only** (S2) | `access_vis.rs::compose_unfinished`; `vm.rs::access_vis` | **landed** |
| 2 | PE unpublished-RAW refuse Execute (S1 / §7) | `ready_edge.rs`; `scheduler.rs::try_execute_ready`; ESTIMATE `vm.rs::note_unpublished_raw`; abort `executor.rs::validate_occ_kernel` | **landed** |
| 3 | `note_fence` only after successful verb (T1/T2) | `certificate.rs::note_success`; `vm.rs::note_fence_success` after Data confirm / WaitFor / lane park | **landed** |
| 4 | SerialLane = exclusive progress token (T4) | `lane.rs`; `vm.rs::pcc_serial_lane` parks / WaitFor — **no** `occ_unfenced` | **landed** |
| 5 | Certificates = access-prefix strips | `certificate.rs::covers_all`; `repair.rs::repair_grain`; `pevm.rs` validate dispatch | **landed** |
| 6 | Quiet + empty PE ⇒ byte-identical OCC (T6) | `vm.rs::specfence_access_gate` — `plant_is_occ` **before** detect / `access_log.note` | **landed** |
| 7 | Cost-aware prior-PE Fire (T3) | `learner.rs::prior_pe_fire_wins`; `access_policy.rs::decide` | **landed** |
| 8 | HotSet/WŜ → posterior / ready-edge priors | `learner.rs::note_hot_ws_posterior`; ESTIMATE + vis gather | **landed** |
| 9 | Independence Unfence stale PE (FM9) | `decide` + `AccessVis.independence_certified` | **landed** |
| 10 | R1 at Fenced fail-\(a\); Spec-only → B0 | `specfence_r1_validate`; mixed sibling Spec → B0 | **landed** |
| 11 | Schedule ready = PE-satisfied ∪ Validate ∪ Repair | `computer.rs::next_sf_task` | **landed** |
| 12 | Soft=0 | held | **held** |

`kernel.rs` is a rem-legal **mirror** (`note_fence` after strip success). Not Mode SoT.

---

## Control loop (live)

```
quiet empty PE:
  maybe_wait → Ok(())          # no detect, no HashMap ordinal, no vis
  validate → OCC bool + B0

PE nonempty / learning arm (ESTIMATE or abort):
  k := access_log.note(ℓ)
  vis := compose_unfinished(!done only)
  decide → Spec | Bind | WaitFor | SerialLane
  Bind: cert only after last_data_before
  SerialLane: grant token + refuse ready-set + park/WaitFor (never Spec continue)
  ESTIMATE Blocking → mark PE + ready_edge(producer)

validate:
  no strip → OCC B0 + PE(true k)
  fail ⊆ strip → R1 museum
  mixed / Spec fail → B0
```

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release` | **183 ok** (was 168) |
| `cargo +nightly test -p pevm --test specfence` | *after this cut* |
| erc20 / raw_transfers / mixed / uniswap / beneficiary / small_blocks | *after this cut* |

New unit: `compose_unfinished` S2; certificate strip not tx-global; lane token; ready-edge refuse; decide prior-only quiet roi_skip; independence Unfence; repair grain B0 vs R1.

Toolchain: `cargo +nightly` (edition 2024), `--config 'profile.release.lto=false'` for local release.

---

## Honesty (fill after sweep)

Sweeps: `lab/results/v6-essence-all-blocks-sweep.json`, `lab/results/v6-essence-focus-n3-sweep.json`.  
Digest: `lab/notes/v6-essence-sweep-summary.json`.

| | This cut | v5 honesty | PC file |
|--|----------|-----------|---------|
| nonempty median SF/OCC | TBD | **0.655** / rem **0.734** | **0.744** / ~0.80 |
| 14689597 N=3 | TBD | 0.327 | — |
| 2179522 N=3 | TBD | 0.751 digest | — |
| 19807137 N=3 | TBD | 0.301 | — |
| Soft / await | 0 / 0 | 0 / 0 | 0 / 0 |

**Do not celebrate structure counters without wall.**
