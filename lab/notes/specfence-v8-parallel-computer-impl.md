# SpecFence v8 — PC ⊗ CC parallel computer implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v8-pc-cc-computer-f6cf`  
**Base:** `cursor/specfence-v6-essence-af82` @ `d6e77f2` (v8 PC⊗CC SoT) / plant `3376ac4`  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` (parent `d6e77f2`, **PC⊗CC co-equal**)  
**Land brief:** `lab/notes/specfence-v8-land-brief.md`  
**Frame:** PC and CC are **first-class peers**. Not “PC primary / CC annotates edges.” Official SoT absorbed from `origin/cursor/specfence-v6-essence-af82`.

**Honesty bar:** nonempty median SF/OCC **> 0.744**; Soft=0; seq≡par. Stretch: 14689597 ≥0.85 @8 N≥3; quiet p10 ≥0.85.

---

## Dual plane (landed)

| Plane | Live modules | Role |
|-------|--------------|------|
| **PC** | `computer.rs`, `producer_stage.rs`, scheduler ready-set | Stages, steal, ProducerStage reserve/promote, wall |
| **CC** | `access_policy.rs`, `access_vis.rs`, `access_log.rs`, `certificate.rs`, `repair.rs`, `lane.rs`, `learner.rs` | Detect/Avoid/Resolve, Mode(a), PE, certs, R1 |
| **Fuse** | `ready_edge.rs`, `executor.rs`, `vm.rs` gate, `pevm.rs` loop | Ready-set law; publish→wake; validate split |

---

## Land map (all v7 gaps, no P0/P1/P2)

| SoT item | file:fn | Live |
|----------|---------|------|
| PC ⊗ CC peer frame | SoT v8 §0.1; `mod.rs` | yes |
| Empty-PE OCC (T6) | `specfence_plant_is_occ` / gate early return; **quiet_fence_off also OCC computer** | **kept** |
| ProducerStage-safe refuse | `producer_stage.rs`; `try_execute_ready` refuse iff producer Executing; `next_sf_task` drops Aborting reservations | yes |
| ReadyEdge + predicted RAW tip | `ready_edge.rs` | yes |
| Bind rare (tip==conflict ∧ EV) | `access_policy::decide`; `AccessVis.tip_is_conflict_producer` | yes |
| WaitFor / SerialLane primary | `decide`; `pcc_wait_for_writer`; `pcc_serial_lane` | yes |
| SerialLane exclusive | `pcc_serial_lane` WaitFor if executing; Ready = canary + ProducerStage/edge (Ready-park/refuse yield-spins) | yes |
| Live true-\(k\) when PE-on | `vm.rs` `access_log.note` on PE-on stream | yes |
| No fan_out templates `[1,6,10,20]` | `learner::note_abort_access` → `arm_location_any_k` | yes |
| Validate split → R1a | `executor::validate_specfence` (covers_all + selective fenced rebind) | yes |
| Cert strips survive WaitFor resume | `certificate::begin_execute` keeps locs on inc>0 | yes |
| Spec-only → B0 | `validate_occ_kernel` when no strip | yes |
| HotSet/WŜ → edges + posterior | `access_vis` insert ReadyEdge; `note_hot_ws_posterior` | yes |
| process.record every Fence verb | Bind / WaitFor / SerialLane in `vm.rs` | yes |
| Bind↑∧abort↓ → roi_skip Bind | `learner::bind_tax_losing` | yes |
| Soft=0 | held | yes |

---

## Control loop (live)

```
empty PE:
  schedule = next_occ_task
  execute  = try_execute(wave=None)
  access   = Ok(())          # no detect, no ordinal, no vis
  validate = validate_occ_kernel

PE nonempty:
  schedule = next_sf_task    # ProducerStage first; PE-refuse iff producer runnable
  execute  = try_execute(wave, fence)
  PE-on: k = access_log.note(ℓ) on every access
         if location_predicted: vis + decide
           WaitFor(executing) → park + cert + process.record
           SerialLane(executing) → grant + WaitFor
           SerialLane(Ready) → reserve ProducerStage + canary (Ready-park yield-spins)
           Bind(tip==RAW producer ∧ EV) → OCC read + cert + process.record
           else Spec ≡ OCC
  validate = validate_specfence
             no cert → OCC B0 + PE(true k)
             covers_all → R1a rebind else B0
```

---

## Tests / honesty

- lib `specfence`: **188 passed**
- `--test specfence`: **42 passed**, 20 ignored; seq≡par held
- Soft=0 on named + all-blocks
- All-blocks N=1 @8 (98 nonempty, tip `a8426c2` quiet-OCC): median **0.744** (bar is **>** 0.744 — not claimed); quiet median **0.963**, p10 **0.518**
- **14689597** N=3 @8: **0.554** (v6 0.336); Bind **7** / Wait **67**; R1=0; aborts 98 vs OCC 71
- **14689597** N=1 @8: **0.440**; Bind **11** / Wait **48**; aborts 61 vs OCC 64
- Bind-flood trip `abort>0` **reverted** (0.199 / 965 aborts)

JSON gitignored under `lab/results/v8-*-quietocc.json`.
