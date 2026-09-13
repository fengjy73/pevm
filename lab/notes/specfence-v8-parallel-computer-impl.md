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
| Empty-PE OCC (T6) | `specfence_plant_is_occ` / gate early return | **kept** |
| ProducerStage-safe refuse | `producer_stage.rs`; `scheduler::try_execute_ready` (Ready/Executing/Validated); `computer::next_sf_task` drops Aborting reservations | yes |
| ReadyEdge + predicted RAW tip | `ready_edge.rs` | yes |
| Bind rare (tip==conflict ∧ EV) | `access_policy::decide`; `AccessVis.tip_is_conflict_producer` | yes |
| WaitFor / SerialLane primary | `decide`; `pcc_wait_for_writer`; `pcc_serial_lane` | yes |
| SerialLane exclusive | `pcc_serial_lane` WaitFor if executing; Ready/Validated = BlockingOther steal (never `occ_unfenced`) | yes |
| Live true-\(k\) when PE-on | `vm.rs` `access_log.note` on PE-on stream | yes |
| No fan_out templates `[1,6,10,20]` | `learner::note_abort_access` → `arm_location_any_k` | yes |
| Validate split → R1a | `executor::validate_specfence` | yes |
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
           SerialLane(Ready) → reserve ProducerStage + Blocking (never occ_unfenced)
           Bind(tip==RAW producer ∧ EV) → OCC read + cert + process.record
           else Spec ≡ OCC
  validate = validate_specfence
             no cert → OCC B0 + PE(true k)
             covers_all → R1a rebind else B0
```

---

## Tests / honesty

See PR body after sweeps. Unit/lib tests must stay green. Stretch bars reported honestly.
