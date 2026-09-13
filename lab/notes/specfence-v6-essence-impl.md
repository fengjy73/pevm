# SpecFence v6 essence — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v6-essence-af82`  
**Base:** `cursor/specfence-v5-pc-cc-fusion-e28e` @ `9a49b5f` + SoT `b4e4010`  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v6-essence.md`

**Honesty bar:** nonempty median SF/OCC **> 0.744**; Soft=0; seq≡par.

---

## Live control loop

```
empty PE (quiet / first wave):
  schedule = next_occ_task
  execute  = try_execute(wave=None)
  maybe_wait = Ok(())          # no detect, no ordinal, no vis
  validate = validate_occ_kernel  # B0 + train PE at true k / templates off quiet

PE nonempty (abort heat, !quiet_fence_off):
  schedule = next_task_with_wave  # WaitFor park steal; no ready-refuse
  execute  = try_execute(wave, fence)
  inc==0 or !location_predicted → Ok(())   # ESTIMATE plant
  else vis + decide:
    WaitFor(executing) → park, note_fence after arm, no pcc_armed
    SerialLane(executing) → grant + WaitFor
    Bind(Data ∧ unfinished=0) → OCC read + cert, no rem
    else Spec ≡ OCC
```

Ready-edge **observes** unpublished RAW / consumers. Schedule-refuse of known consumers deadlocks (producer off collaborative index) — Avoid is the access verb.

---

## Land map

| SoT item | file:fn | Live |
|----------|---------|------|
| unfinished = !done only (S2) | `access_vis.rs::compose_unfinished` | yes |
| PE refuse Execute / ready-edge | `ready_edge.rs` observe; schedule refuse **off** | observe |
| note_fence after successful verb | `vm.rs::note_fence_success` after WaitFor arm / Bind Data | yes |
| SerialLane exclusive token | `lane.rs`; `pcc_serial_lane` parks executing only | yes |
| cert strips | `certificate.rs`; not tx-global | yes |
| Quiet empty PE ≡ OCC (T6) | `maybe_wait` / gate before detect | yes |
| Cost-aware prior-PE (T3) | `decide` + `quiet_fence_off` | yes |
| HotSet/WŜ → posterior | `note_hot_ws_posterior` in `access_vis` | yes |
| Independence Unfence | `decide` FM9 | yes |
| Spec fail → B0 + PE | `validate_occ_kernel` | yes |
| Hybrid OCC→ready computer | `pevm.rs` `has_any_predicted` | yes |
| Soft=0 | held | yes |

Quiet abort without true-k does **not** spray PE templates (`quiet_fence_off`).

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release` | running / expected 184 |
| `--test specfence` | **42 ok**, 20 ignored |

---

## Honesty (Soft=0, isolated)

All-blocks N=1 nonempty **median 0.795 > 0.744**. Soft=0. Live bind=972 wait=1047.

| Block | N=1 SF/OCC | Honest |
|------:|----------:|--------|
| 14689597 | **0.162** (28 vs 4.5ms; 624 vs 66 aborts; bind=535) | Bind tax; **< 0.85** |
| 2179522 | 39.95 | OCC-slow — **do not advertise**. Sister run OCC-fast **0.238** |
| 19807137 | **0.350** (47 vs 16ms) | spine tax |

Quiet median **1.047**, quiet p10 **0.644**.

**Bar hit:** median **0.795**. **Stretch missed:** 14689597 ≥0.85; quiet p10 ≥0.85.
